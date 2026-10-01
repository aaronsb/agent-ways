use anyhow::{Context, Result};
use rust_stemmers::{Algorithm, Stemmer};
use std::collections::HashMap;

// Minimal stopword list for vocabulary suggestions (authoring-time only,
// not part of the matcher — ADR-125 made embedding the sole retrieval tier).
const STOPWORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being",
    "have", "has", "had", "do", "does", "did", "will", "would", "could",
    "should", "may", "might", "must", "shall", "can", "this", "that",
    "these", "those", "it", "its", "what", "how", "why", "when", "where",
    "who", "let", "lets", "just", "to", "for", "of", "in", "on", "at",
    "by", "and", "or", "but", "not", "with", "from", "into", "about",
    "than", "then", "so", "if", "up", "out", "no", "yes", "all", "some",
    "any", "each", "my", "your", "our", "me", "we", "you", "i",
];

fn tokenize(text: &str, stemmer: &Stemmer) -> Vec<String> {
    tokenize_pairs(text, stemmer).into_iter().map(|(s, _)| s).collect()
}

pub fn run(file: String, min_freq: u32) -> Result<()> {
    let content = std::fs::read_to_string(&file)
        .with_context(|| format!("reading {file}"))?;

    let (description, vocabulary, body) = parse_way_file(&content)
        .with_context(|| format!("parsing {file}"))?;

    let stemmer = Stemmer::create(Algorithm::English);

    // Tokenize body with original forms preserved
    let body_pairs = tokenize_pairs(&body, &stemmer);

    // Build body term frequency map (stem → (best_original, freq))
    let mut body_tf: HashMap<String, (String, u32)> = HashMap::new();
    for (stem, original) in &body_pairs {
        let entry = body_tf.entry(stem.clone()).or_insert_with(|| (original.clone(), 0));
        entry.1 += 1;
        // Keep longest original form (most readable)
        if original.len() > entry.0.len() {
            entry.0 = original.clone();
        }
    }

    // Mark body terms covered by description + vocabulary
    let covered_text = format!("{description} {vocabulary}");
    let covered_stems: std::collections::HashSet<String> =
        tokenize(&covered_text, &stemmer).into_iter().collect();

    // Find vocabulary terms not appearing in body
    let vocab_words: Vec<&str> = vocabulary.split_whitespace().collect();
    let mut unused: Vec<&str> = Vec::new();
    for word in &vocab_words {
        let lower: String = word.to_lowercase();
        let stem = stemmer.stem(&lower).to_string();
        if !body_tf.contains_key(&stem) {
            unused.push(word);
        }
    }

    // Sort by frequency descending
    let mut entries: Vec<(&String, &(String, u32))> = body_tf.iter().collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.1 .1));

    // Output: GAPS
    let mut gap_count = 0;
    println!("GAPS");
    for (stem, (original, freq)) in &entries {
        if !covered_stems.contains(*stem) && *freq >= min_freq {
            println!("{original}\t{freq}\t{stem}");
            gap_count += 1;
        }
    }

    // Output: COVERAGE
    println!("COVERAGE");
    for (stem, (original, freq)) in &entries {
        if covered_stems.contains(*stem) {
            println!("{original}\t{freq}\t{stem}");
        }
    }

    // Output: UNUSED
    println!("UNUSED");
    for word in &unused {
        println!("{word}");
    }

    // Output: VOCABULARY (current + gaps)
    print!("VOCABULARY\n{vocabulary}");
    for (stem, (original, freq)) in &entries {
        if !covered_stems.contains(*stem) && *freq >= min_freq {
            print!(" {original}");
        }
    }
    println!();

    let covered_count = entries.iter().filter(|(s, _)| covered_stems.contains(*s)).count();
    eprintln!(
        "suggest: {gap_count} gaps (min_freq={min_freq}), {covered_count} covered, {} unused",
        unused.len()
    );

    if gap_count > 0 {
        std::process::exit(0);
    } else {
        std::process::exit(1);
    }
}

fn parse_way_file(content: &str) -> Option<(String, String, String)> {
    if !crate::util::has_frontmatter(content) {
        return None;
    }

    let fm_end = content[4..].find("\n---\n").or_else(|| content[4..].find("\n---"))?;
    let fm = &content[4..4 + fm_end];
    let body_start = 4 + fm_end + 4; // skip \n---\n
    let body = if body_start < content.len() {
        &content[body_start..]
    } else {
        ""
    };

    let mut description = String::new();
    let mut vocabulary = String::new();

    for line in fm.lines() {
        if let Some(val) = line.strip_prefix("description:") {
            description = val.trim().to_string();
        }
        if let Some(val) = line.strip_prefix("vocabulary:") {
            vocabulary = val.trim().to_string();
        }
    }

    Some((description, vocabulary, body.to_string()))
}

/// Tokenize text preserving original form alongside stem.
fn tokenize_pairs(text: &str, stemmer: &Stemmer) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        if ch.is_alphabetic() {
            current.extend(ch.to_lowercase());
        } else if !current.is_empty() {
            if current.len() >= 3 && !STOPWORDS.contains(&current.as_str()) {
                let stemmed = stemmer.stem(&current).to_string();
                if stemmed.len() >= 3 {
                    pairs.push((stemmed, current.clone()));
                }
            }
            current.clear();
        }
    }
    if current.len() >= 3 && !STOPWORDS.contains(&current.as_str()) {
        let stemmed = stemmer.stem(&current).to_string();
        if stemmed.len() >= 3 {
            pairs.push((stemmed, current));
        }
    }
    pairs
}
