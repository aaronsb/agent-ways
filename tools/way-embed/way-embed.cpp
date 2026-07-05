/*
 * way-embed: Embedding-based semantic matcher for the ways system
 *
 * Uses all-MiniLM-L6-v2 (via llama.cpp) to embed way descriptions and
 * user prompts, scoring by cosine similarity.
 *
 * Two modes:
 *   generate - embed corpus descriptions, write vectors to JSONL
 *   match    - embed query, score against pre-computed vectors
 *
 * See: ADR-108
 */

#include "common.h"
#include "log.h"
#include "llama.h"

#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>
#include <algorithm>

#define VERSION "0.1.0"
#define MAX_CORPUS 512
#define MAX_LINE   65536  /* corpus lines can be long with embedding vectors */

/* ========================================================================
 * Minimal JSON helpers (no dependency on a JSON library)
 * ======================================================================== */

/* Extract a string value for a given key from a JSON object line.
 * Returns empty string if key not found. Handles escaped quotes. */
static std::string json_get_string(const char *json, const char *key) {
    char pattern[256];
    snprintf(pattern, sizeof(pattern), "\"%s\":\"", key);
    const char *start = strstr(json, pattern);
    if (!start) {
        /* try with space after colon */
        snprintf(pattern, sizeof(pattern), "\"%s\": \"", key);
        start = strstr(json, pattern);
    }
    if (!start) return "";

    start = strchr(start + strlen(key) + 2, '"');
    if (!start) return "";
    start++; /* skip opening quote */

    std::string result;
    for (const char *p = start; *p && *p != '"'; p++) {
        if (*p == '\\' && *(p + 1)) {
            p++;
            if (*p == '"') result += '"';
            else if (*p == '\\') result += '\\';
            else if (*p == 'n') result += '\n';
            else { result += '\\'; result += *p; }
        } else {
            result += *p;
        }
    }
    return result;
}

/* Extract a numeric value for a given key. Returns default_val if not found. */
static double json_get_number(const char *json, const char *key, double default_val) {
    char pattern[256];
    snprintf(pattern, sizeof(pattern), "\"%s\":", key);
    const char *start = strstr(json, pattern);
    if (!start) return default_val;

    start += strlen(pattern);
    while (*start == ' ') start++;

    char *end;
    double val = strtod(start, &end);
    if (end == start) return default_val;
    return val;
}

/* Extract a float array for a given key. Returns empty vector if not found. */
static std::vector<float> json_get_float_array(const char *json, const char *key) {
    char pattern[256];
    snprintf(pattern, sizeof(pattern), "\"%s\":[", key);
    const char *start = strstr(json, pattern);
    if (!start) {
        snprintf(pattern, sizeof(pattern), "\"%s\": [", key);
        start = strstr(json, pattern);
    }
    if (!start) return {};

    start = strchr(start + strlen(key) + 2, '[');
    if (!start) return {};
    start++; /* skip '[' */

    std::vector<float> result;
    while (*start && *start != ']') {
        while (*start == ' ' || *start == ',') start++;
        if (*start == ']') break;
        char *end;
        float val = strtof(start, &end);
        if (end == start) break;
        result.push_back(val);
        start = end;
    }
    return result;
}

/* Escape a string for JSON output */
static std::string json_escape(const std::string &s) {
    std::string out;
    for (char c : s) {
        if (c == '"') out += "\\\"";
        else if (c == '\\') out += "\\\\";
        else if (c == '\n') out += "\\n";
        else out += c;
    }
    return out;
}

/* ========================================================================
 * Corpus data structures
 * ======================================================================== */

struct corpus_entry {
    std::string id;
    std::string description;
    std::string vocabulary;
    double threshold;         /* BM25 threshold */
    double embed_threshold;   /* cosine similarity threshold */
    std::vector<float> embedding; /* 384-dim pre-computed vector */
    std::string raw_line;     /* original JSON line for passthrough */
};

static int load_corpus(const char *path, std::vector<corpus_entry> &corpus) {
    FILE *f = fopen(path, "r");
    if (!f) {
        fprintf(stderr, "error: cannot open corpus: %s\n", path);
        return -1;
    }

    char *line = (char *)malloc(MAX_LINE);
    if (!line) { fclose(f); return -1; }

    while (fgets(line, MAX_LINE, f)) {
        /* skip empty lines */
        if (line[0] == '\n' || line[0] == '\0') continue;

        corpus_entry entry;
        entry.id = json_get_string(line, "id");
        entry.description = json_get_string(line, "description");
        entry.vocabulary = json_get_string(line, "vocabulary");
        entry.threshold = json_get_number(line, "threshold", 2.0);
        entry.embed_threshold = json_get_number(line, "embed_threshold", 0.35);
        entry.embedding = json_get_float_array(line, "embedding");

        /* strip trailing newline from raw line */
        size_t len = strlen(line);
        while (len > 0 && (line[len-1] == '\n' || line[len-1] == '\r')) {
            line[--len] = '\0';
        }
        entry.raw_line = line;

        if (!entry.id.empty()) {
            corpus.push_back(std::move(entry));
        }
    }

    free(line);
    fclose(f);
    return 0;
}

/* ========================================================================
 * Embedding engine (wraps llama.cpp)
 * ======================================================================== */

struct embed_engine {
    llama_model *model;
    llama_context *ctx;
    const llama_vocab *vocab;
    int n_embd;
};

static embed_engine *engine_init(const char *model_path) {
    auto *engine = new embed_engine();

    llama_backend_init();

    /* model params: minimal, no GPU */
    auto mparams = llama_model_default_params();
    mparams.n_gpu_layers = 0;

    engine->model = llama_model_load_from_file(model_path, mparams);
    if (!engine->model) {
        fprintf(stderr, "error: failed to load model: %s\n", model_path);
        delete engine;
        llama_backend_free();
        return nullptr;
    }

    engine->vocab = llama_model_get_vocab(engine->model);
    engine->n_embd = llama_model_n_embd(engine->model);

    /* context params: small context for short texts */
    auto cparams = llama_context_default_params();
    cparams.n_ctx = 512;  /* MiniLM max is 256 tokens, but padding helps */
    cparams.n_batch = 512;
    cparams.n_ubatch = 512;
    cparams.embeddings = true;
    cparams.pooling_type = LLAMA_POOLING_TYPE_MEAN;
    cparams.n_threads = 2;

    engine->ctx = llama_init_from_model(engine->model, cparams);
    if (!engine->ctx) {
        fprintf(stderr, "error: failed to create context\n");
        llama_model_free(engine->model);
        delete engine;
        llama_backend_free();
        return nullptr;
    }

    return engine;
}

static void engine_free(embed_engine *engine) {
    if (!engine) return;
    if (engine->ctx) llama_free(engine->ctx);
    if (engine->model) llama_model_free(engine->model);
    delete engine;
    llama_backend_free();
}

/* Embed a single text, return normalized vector */
static std::vector<float> engine_embed(embed_engine *engine, const std::string &text) {
    std::vector<float> result(engine->n_embd, 0.0f);

    /* tokenize */
    std::vector<llama_token> tokens = common_tokenize(engine->ctx, text, true, true);
    if (tokens.empty()) return result;

    /* truncate to context size */
    int n_ctx = llama_n_ctx(engine->ctx);
    if ((int)tokens.size() > n_ctx) {
        tokens.resize(n_ctx);
    }

    /* prepare batch */
    struct llama_batch batch = llama_batch_init(tokens.size(), 0, 1);
    common_batch_clear(batch);
    for (size_t i = 0; i < tokens.size(); i++) {
        common_batch_add(batch, tokens[i], i, { 0 }, true);
    }

    /* clear KV cache */
    llama_memory_clear(llama_get_memory(engine->ctx), true);

    /* decode */
    if (llama_decode(engine->ctx, batch) < 0) {
        fprintf(stderr, "error: llama_decode failed\n");
        llama_batch_free(batch);
        return result;
    }

    /* extract embedding (mean pooled by llama.cpp) */
    const float *embd = llama_get_embeddings_seq(engine->ctx, 0);
    if (!embd) {
        fprintf(stderr, "error: failed to get embeddings\n");
        llama_batch_free(batch);
        return result;
    }

    /* normalize (L2) */
    common_embd_normalize(embd, result.data(), engine->n_embd, 2);

    llama_batch_free(batch);
    return result;
}

/* ========================================================================
 * Cosine similarity
 * ======================================================================== */

static float cosine_similarity(const float *a, const float *b, int n) {
    /* vectors are already L2-normalized, so dot product = cosine similarity */
    float dot = 0.0f;
    for (int i = 0; i < n; i++) {
        dot += a[i] * b[i];
    }
    return dot;
}

/* ========================================================================
 * Commands
 * ======================================================================== */

/* similarity: embed two texts, print cosine similarity.
 * If text1 and text2 are provided, single-pair mode.
 * If --batch, reads TAB-separated pairs from stdin (one per line),
 * loads model once, prints one similarity score per line. */
static int cmd_similarity(const char *model_path, const char *text1, const char *text2, bool batch) {
    embed_engine *engine = engine_init(model_path);
    if (!engine) return 1;

    if (batch) {
        /* Batch mode: read TAB-separated pairs from stdin */
        char line[8192];
        while (fgets(line, sizeof(line), stdin)) {
            /* strip newline */
            size_t len = strlen(line);
            while (len > 0 && (line[len-1] == '\n' || line[len-1] == '\r'))
                line[--len] = '\0';
            if (len == 0) continue;

            /* split on TAB */
            char *tab = strchr(line, '\t');
            if (!tab) {
                fprintf(stderr, "error: batch line missing TAB separator\n");
                continue;
            }
            *tab = '\0';
            const char *t1 = line;
            const char *t2 = tab + 1;

            auto vec1 = engine_embed(engine, t1);
            auto vec2 = engine_embed(engine, t2);
            float sim = cosine_similarity(vec1.data(), vec2.data(), engine->n_embd);
            printf("%.4f\n", sim);
            fflush(stdout);
        }
    } else {
        auto vec1 = engine_embed(engine, text1);
        auto vec2 = engine_embed(engine, text2);
        float sim = cosine_similarity(vec1.data(), vec2.data(), engine->n_embd);
        printf("%.4f\n", sim);
    }

    engine_free(engine);
    return 0;
}

/* generate: embed corpus descriptions, write augmented JSONL */
static int cmd_generate(const char *corpus_path, const char *model_path, const char *output_path) {
    std::vector<corpus_entry> corpus;
    if (load_corpus(corpus_path, corpus) < 0) return 1;

    embed_engine *engine = engine_init(model_path);
    if (!engine) return 1;

    const char *out_path = output_path ? output_path : corpus_path;
    std::string tmp_path = std::string(out_path) + ".tmp";
    FILE *out = fopen(tmp_path.c_str(), "w");
    if (!out) {
        fprintf(stderr, "error: cannot write to: %s\n", tmp_path.c_str());
        engine_free(engine);
        return 1;
    }

    int count = 0;
    for (auto &entry : corpus) {
        /* embed description + vocabulary */
        std::string text = entry.description + " " + entry.vocabulary;
        std::vector<float> vec = engine_embed(engine, text);

        /* write JSON line with all original fields + embedding */
        fprintf(out, "{\"id\":\"%s\",\"description\":\"%s\",\"vocabulary\":\"%s\","
                     "\"threshold\":%g,\"embed_threshold\":%g,\"embedding\":[",
                json_escape(entry.id).c_str(),
                json_escape(entry.description).c_str(),
                json_escape(entry.vocabulary).c_str(),
                entry.threshold,
                entry.embed_threshold);

        for (int i = 0; i < engine->n_embd; i++) {
            if (i > 0) fputc(',', out);
            fprintf(out, "%.6f", vec[i]);
        }
        fprintf(out, "]}\n");
        count++;

        fprintf(stderr, "  [%d/%d] %s\n", count, (int)corpus.size(), entry.id.c_str());
    }

    fclose(out);

    /* atomic rename — on Windows, rename() fails if destination exists; remove first */
#ifdef _WIN32
    remove(out_path);
#endif
    if (rename(tmp_path.c_str(), out_path) != 0) {
        fprintf(stderr, "error: failed to rename %s -> %s\n", tmp_path.c_str(), out_path);
        engine_free(engine);
        return 1;
    }

    fprintf(stderr, "Generated %s: %d ways with %d-dim embeddings\n",
            out_path, count, engine->n_embd);

    engine_free(engine);
    return 0;
}

/* Score one query against the pre-loaded corpus and emit its matches.
 * qindex < 0  → single-query mode: output "id<TAB>score" (unchanged format).
 * qindex >= 0 → batch mode: output "qindex<TAB>id<TAB>score" so a caller can
 * group a stream of results back to the query that produced them. */
static void score_query(embed_engine *engine,
                        const std::vector<corpus_entry> &corpus,
                        const std::string &query,
                        double default_threshold,
                        int qindex) {
    std::vector<float> query_vec = engine_embed(engine, query);
    int n_embd = engine->n_embd;

    struct match_result {
        std::string id;
        float score;
    };
    std::vector<match_result> matches;

    for (const auto &entry : corpus) {
        if ((int)entry.embedding.size() != n_embd) continue;

        float score = cosine_similarity(query_vec.data(), entry.embedding.data(), n_embd);
        double thresh = (default_threshold >= 0) ? default_threshold : entry.embed_threshold;

        if (score >= thresh) {
            matches.push_back({entry.id, score});
        }
    }

    /* sort by score descending */
    std::sort(matches.begin(), matches.end(),
              [](const match_result &a, const match_result &b) {
                  return a.score > b.score;
              });

    for (const auto &m : matches) {
        if (qindex >= 0) printf("%d\t%s\t%.4f\n", qindex, m.id.c_str(), m.score);
        else             printf("%s\t%.4f\n", m.id.c_str(), m.score);
    }
}

/* match: embed query, score against pre-computed corpus vectors.
 * Single-query mode scores --query. Batch mode (--batch) loads the model and
 * corpus once, then reads one query per line from stdin — the batched-embedding
 * primitive that amortizes the ~22ms model load across every chunk a hook needs
 * to match in a single invocation (ADR-160). */
static int cmd_match(const char *corpus_path, const char *model_path, const char *query,
                     double default_threshold, bool batch) {
    std::vector<corpus_entry> corpus;
    if (load_corpus(corpus_path, corpus) < 0) return 1;

    /* verify corpus has embeddings */
    int has_embeddings = 0;
    for (const auto &entry : corpus) {
        if (!entry.embedding.empty()) has_embeddings++;
    }
    if (has_embeddings == 0) {
        fprintf(stderr, "error: corpus has no embeddings. Run 'way-embed generate' first.\n");
        return 1;
    }

    embed_engine *engine = engine_init(model_path);
    if (!engine) return 1;

    if (batch) {
        /* One query per stdin line. Blank lines are skipped and do NOT advance
         * the index, so callers must send only non-empty, single-line queries
         * (flatten any embedded newlines) to keep qindex aligned with input. */
        char *line = (char *)malloc(MAX_LINE);
        if (!line) { engine_free(engine); return 1; }
        int qindex = 0;
        while (fgets(line, MAX_LINE, stdin)) {
            size_t len = strlen(line);
            while (len > 0 && (line[len-1] == '\n' || line[len-1] == '\r'))
                line[--len] = '\0';
            if (len == 0) continue;
            score_query(engine, corpus, line, default_threshold, qindex);
            fflush(stdout);
            qindex++;
        }
        free(line);
    } else {
        score_query(engine, corpus, query, default_threshold, -1);
    }

    engine_free(engine);
    return 0;
}

/* ========================================================================
 * CLI
 * ======================================================================== */

static void usage(const char *prog) {
    fprintf(stderr,
        "way-embed v%s — embedding-based semantic matcher for ways\n\n"
        "Usage:\n"
        "  %s generate --corpus FILE --model FILE [--output FILE]\n"
        "    Embed all corpus descriptions, write vectors to JSONL.\n\n"
        "  %s match --corpus FILE --model FILE (--query TEXT | --batch) [--threshold N]\n"
        "    Score query against pre-computed corpus embeddings.\n"
        "    Single: --query TEXT   → id<TAB>score per match above threshold.\n"
        "    Batch:  --batch        → read one query per line from stdin, load\n"
        "            model+corpus once, emit qindex<TAB>id<TAB>score per match.\n\n"
        "  %s similarity --model FILE --text1 TEXT --text2 TEXT\n"
        "    Embed two texts and print their cosine similarity.\n\n"
        "Options:\n"
        "  --corpus FILE     Path to ways-corpus.jsonl\n"
        "  --model FILE      Path to GGUF model file\n"
        "  --query TEXT      User prompt to match\n"
        "  --threshold N     Override per-way embed_threshold (default: use per-way)\n"
        "  --output FILE     Output path for generate (default: overwrite corpus)\n"
        "  --text1 TEXT      First text for similarity comparison\n"
        "  --text2 TEXT      Second text for similarity comparison\n"
        "  --version         Print version\n",
        VERSION, prog, prog, prog);
}

int main(int argc, char **argv) {
    if (argc < 2) {
        usage(argv[0]);
        return 1;
    }

    if (strcmp(argv[1], "--version") == 0) {
        printf("way-embed %s\n", VERSION);
        return 0;
    }

    const char *command = argv[1];
    const char *corpus_path = nullptr;
    const char *model_path = nullptr;
    const char *query = nullptr;
    const char *output_path = nullptr;
    const char *text1 = nullptr;
    const char *text2 = nullptr;
    bool batch = false;
    double threshold = -1.0; /* negative = use per-way */

    for (int i = 2; i < argc; i++) {
        if (strcmp(argv[i], "--corpus") == 0 && i + 1 < argc) {
            corpus_path = argv[++i];
        } else if (strcmp(argv[i], "--model") == 0 && i + 1 < argc) {
            model_path = argv[++i];
        } else if (strcmp(argv[i], "--query") == 0 && i + 1 < argc) {
            query = argv[++i];
        } else if (strcmp(argv[i], "--threshold") == 0 && i + 1 < argc) {
            threshold = atof(argv[++i]);
        } else if (strcmp(argv[i], "--output") == 0 && i + 1 < argc) {
            output_path = argv[++i];
        } else if (strcmp(argv[i], "--text1") == 0 && i + 1 < argc) {
            text1 = argv[++i];
        } else if (strcmp(argv[i], "--text2") == 0 && i + 1 < argc) {
            text2 = argv[++i];
        } else if (strcmp(argv[i], "--batch") == 0) {
            batch = true;
        } else if (strcmp(argv[i], "--version") == 0) {
            printf("way-embed %s\n", VERSION);
            return 0;
        } else {
            fprintf(stderr, "error: unknown option: %s\n", argv[i]);
            return 1;
        }
    }

    /* suppress all llama.cpp logging (model loader uses ggml_log, not common_log) */
    llama_log_set([](enum ggml_log_level, const char *, void *) {}, nullptr);
    common_log_set_verbosity_thold(-1);

    if (strcmp(command, "generate") == 0) {
        if (!corpus_path || !model_path) {
            fprintf(stderr, "error: generate requires --corpus and --model\n");
            return 1;
        }
        return cmd_generate(corpus_path, model_path, output_path);

    } else if (strcmp(command, "match") == 0) {
        if (!corpus_path || !model_path || (!batch && !query)) {
            fprintf(stderr, "error: match requires --corpus, --model, and (--query TEXT | --batch)\n");
            return 1;
        }
        return cmd_match(corpus_path, model_path, query, threshold, batch);

    } else if (strcmp(command, "similarity") == 0) {
        if (!model_path || (!batch && (!text1 || !text2))) {
            fprintf(stderr, "error: similarity requires --model and (--text1 --text2 | --batch)\n");
            return 1;
        }
        return cmd_similarity(model_path, text1, text2, batch);

    } else {
        fprintf(stderr, "error: unknown command: %s (expected 'generate', 'match', or 'similarity')\n", command);
        return 1;
    }
}
