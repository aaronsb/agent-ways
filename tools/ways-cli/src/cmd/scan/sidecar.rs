//! scan/sidecar.rs — the body sidecar (ADR-701 §6, §7).
//!
//! `ways corpus` embeds every way's heading sections
//! ([`super::late_interaction::chunk_sections`]) once and stores the vectors in a
//! binary file beside the alias corpus. Body confirmation then scores a won
//! chunk against those vectors instead of embedding body sentences on every
//! prompt.
//!
//! ## File format (`ways-body-en.bin`, version 1)
//!
//! All integers little-endian, vectors f32 little-endian.
//!
//! ```text
//! magic        8 bytes  "WAYSBODY"
//! version      u32      1
//! dim          u32      embedding dimension (384 for MiniLM)
//! way_count    u32      number of way records
//! vector_count u32      total section vectors (the sum of section_count)
//! model_len    u16      then model_len bytes of UTF-8: the model id
//! way_count records:
//!   id_len     u16      then id_len bytes of UTF-8: the corpus id
//!   hash       u64      content hash of the way file (FNV-1a 64 of its bytes)
//!   sections   u32      section vectors belonging to this way
//! vector_count × dim f32, the ways' vectors in record order
//! ```
//!
//! A file is read only when its length is exactly what its header implies, so
//! a truncated or foreign file reads as absent. Every way in the alias corpus
//! gets a record, disabled or not (ADR-701 §1); a way with no sections has a
//! record with `sections = 0` and confirms against its alias vector.
//!
//! ## Completeness (ADR-701 §7)
//!
//! The corpus manifest (`embed-manifest.json`) records `way_hashes`, each alias
//! corpus way's content hash, and under `body_sidecar` whether the build's
//! way-embed can return chunk vectors (`"vectors": true`, 1.2.0 and later). A
//! scan uses the sidecar only when the manifest says so, its model id is the
//! installed one's (the model, the way-embed binary and [`CHUNKER_REV`]), and
//! every enabled way the alias corpus holds has a
//! record whose hash equals the manifest's. An enabled way the alias corpus
//! lacks (a worktree's or an unregistered project's) cannot win a chunk and
//! does not count. Anything less and the scan behaves exactly as without it.

use std::collections::HashMap;
use std::path::Path;

/// The sidecar's file name in the corpus directory.
pub(crate) const FILE: &str = "ways-body-en.bin";
const MAGIC: &[u8; 8] = b"WAYSBODY";
const VERSION: u32 = 1;

/// One way's section vectors, as the corpus build hands them to [`encode`].
pub(crate) struct WaySections {
    pub id: String,
    pub hash: u64,
    pub vectors: Vec<Vec<f32>>,
}

/// A loaded sidecar.
#[derive(Debug)]
pub(crate) struct Sidecar {
    pub model: String,
    pub dim: usize,
    /// Corpus id to (content hash, first row, row count).
    index: HashMap<String, (u64, usize, usize)>,
    vectors: Vec<f32>,
}

/// Content hash of a way file: FNV-1a 64 over its bytes.
pub(crate) fn content_hash(bytes: &[u8]) -> u64 {
    agent_identity::identity::fnv1a_64(bytes)
}

/// The manifest's spelling of a content hash.
pub(crate) fn hash_hex(hash: u64) -> String {
    format!("{hash:016x}")
}

/// Revision of [`super::late_interaction::chunk_sections`]. Bump it when the
/// chunking changes, so sidecars built by the old chunker stop being used.
pub(crate) const CHUNKER_REV: u32 = 1;

/// What the sidecar's vectors depend on: the English model in `engine_dir`
/// (name and size), the way-embed binary `bin` (size and modification time,
/// read without running it) and the chunker revision. Replacing any of them
/// invalidates the sidecar, so a scan never asks a different binary for
/// `--vectors` or mixes vectors from two embedders.
pub(crate) fn model_id(engine_dir: &Path, bin: &Path) -> Option<String> {
    let model = engine_dir.join(crate::paths::EN_MODEL).metadata().ok()?.len();
    let b = bin.metadata().ok()?;
    let mtime = b.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some(format!("{}:{model}|way-embed:{}:{mtime}|sections:{CHUNKER_REV}", crate::paths::EN_MODEL, b.len()))
}

/// Serialize ways into the version-1 format. `None` when a vector's length is
/// not `dim` or a length overflows its field.
pub(crate) fn encode(model: &str, dim: usize, ways: &[WaySections]) -> Option<Vec<u8>> {
    let vector_count: usize = ways.iter().map(|w| w.vectors.len()).sum();
    let mut out = Vec::with_capacity(64 + ways.len() * 48 + vector_count * dim * 4);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&u32::try_from(dim).ok()?.to_le_bytes());
    out.extend_from_slice(&u32::try_from(ways.len()).ok()?.to_le_bytes());
    out.extend_from_slice(&u32::try_from(vector_count).ok()?.to_le_bytes());
    put_str(&mut out, model)?;
    for w in ways {
        put_str(&mut out, &w.id)?;
        out.extend_from_slice(&w.hash.to_le_bytes());
        out.extend_from_slice(&u32::try_from(w.vectors.len()).ok()?.to_le_bytes());
    }
    for w in ways {
        for v in &w.vectors {
            if v.len() != dim {
                return None;
            }
            for x in v {
                out.extend_from_slice(&x.to_le_bytes());
            }
        }
    }
    Some(out)
}

fn put_str(out: &mut Vec<u8>, s: &str) -> Option<()> {
    out.extend_from_slice(&u16::try_from(s.len()).ok()?.to_le_bytes());
    out.extend_from_slice(s.as_bytes());
    Some(())
}

/// Parse a version-1 sidecar. `None` for any other version, a bad magic, or a
/// length that disagrees with the header.
pub(crate) fn decode(bytes: &[u8]) -> Option<Sidecar> {
    let mut r = Reader { bytes, at: 0 };
    if r.take(8)? != MAGIC || r.u32()? != VERSION {
        return None;
    }
    let dim = r.u32()? as usize;
    let way_count = r.u32()? as usize;
    let vector_count = r.u32()? as usize;
    let model = r.str()?;
    // No capacity from the header: a corrupt count must fail on the bytes it
    // does not have, not allocate what it claims.
    let mut index = HashMap::new();
    let mut row = 0usize;
    for _ in 0..way_count {
        let id = r.str()?;
        let hash = r.u64()?;
        let n = r.u32()? as usize;
        index.insert(id, (hash, row, n));
        row = row.checked_add(n)?;
    }
    if row != vector_count || r.bytes.len() - r.at != vector_count.checked_mul(dim)?.checked_mul(4)? {
        return None;
    }
    let (words, _) = r.bytes[r.at..].as_chunks::<4>();
    let vectors = words.iter().map(|c| f32::from_le_bytes(*c)).collect();
    Some(Sidecar { model, dim, index, vectors })
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.bytes.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_le_bytes(b.try_into().unwrap_or([0; 4])))
    }
    fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|b| u64::from_le_bytes(b.try_into().unwrap_or([0; 8])))
    }
    fn str(&mut self) -> Option<String> {
        let n = self.u16()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
}

/// Write `bytes` to `path` through a sibling staging file, so a reader never
/// sees a half-written sidecar.
pub(crate) fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    agent_settings::writer::write_atomic(path, bytes)
}

/// Read and parse the sidecar at `path`.
pub(crate) fn read(path: &Path) -> Option<Sidecar> {
    decode(&std::fs::read(path).ok()?)
}

/// What a scan needs from the corpus manifest about the sidecar.
#[derive(Debug, Default)]
pub(crate) struct ManifestView {
    /// `way_hashes`: each alias corpus way's content hash. Empty when the
    /// manifest predates the field.
    pub alias: HashMap<String, u64>,
    /// `body_sidecar.vectors`: the build's way-embed returns chunk vectors.
    pub vectors: bool,
    /// `body_sidecar.reason`: why the last build produced no sidecar.
    pub reason: Option<String>,
    /// `body_sidecar.unsupported`: no sidecar because way-embed lacks
    /// `--vectors`, an install state rather than a failure.
    pub unsupported: bool,
}

impl ManifestView {
    pub(crate) fn read(manifest: &Path) -> Self {
        std::fs::read_to_string(manifest)
            .ok()
            .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
            .map(|m| Self::from_value(&m))
            .unwrap_or_default()
    }

    pub(crate) fn from_value(m: &serde_json::Value) -> Self {
        let side = m.get("body_sidecar");
        ManifestView {
            alias: alias_hashes_from(m),
            vectors: side.and_then(|s| s.get("vectors")).and_then(|v| v.as_bool()).unwrap_or(false),
            reason: side.and_then(|s| s.get("reason")).and_then(|v| v.as_str()).map(str::to_string),
            unsupported: side.and_then(|s| s.get("unsupported")).and_then(|v| v.as_bool()).unwrap_or(false),
        }
    }
}

/// ADR-701 §7: the sidecar in `corpus_dir` if a scan may use it, else why not.
/// `bin` is the way-embed the scan runs; `enabled` the ways allowed to compete.
pub(crate) fn state<'a>(
    corpus_dir: &Path,
    bin: &Path,
    enabled: impl IntoIterator<Item = &'a str>,
) -> Result<Sidecar, Fallback> {
    let view = ManifestView::read(&corpus_dir.join("embed-manifest.json"));
    if view.unsupported {
        return Err(Fallback::NoVectors);
    }
    if let Some(why) = view.reason {
        return Err(Fallback::BuildFailed(why));
    }
    let sc = read(&corpus_dir.join(FILE)).ok_or(Fallback::Absent)?;
    if !view.vectors {
        return Err(Fallback::BuiltWithoutVectors);
    }
    let model = model_id(corpus_dir, bin).ok_or(Fallback::ModelMismatch)?;
    sc.check(&model, enabled, &view.alias)?;
    Ok(sc)
}

/// The manifest's `way_hashes` of a parsed manifest.
pub(crate) fn alias_hashes_from(manifest: &serde_json::Value) -> HashMap<String, u64> {
    manifest
        .get("way_hashes")
        .and_then(|v| v.as_object())
        .map(|o| {
            o.iter()
                .filter_map(|(id, h)| Some((id.clone(), u64::from_str_radix(h.as_str()?, 16).ok()?)))
                .collect()
        })
        .unwrap_or_default()
}

/// Why a scan confirms per call instead of against the sidecar (ADR-701 §7).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Fallback {
    /// No way-embed binary: no embedding at all.
    NoEmbedder,
    /// No sidecar file, or one that does not parse.
    Absent,
    /// The last corpus build did not produce a sidecar; its reason.
    BuildFailed(String),
    /// The build's way-embed cannot return chunk vectors (`--vectors`).
    NoVectors,
    /// A sidecar from a build that recorded no chunk vectors.
    BuiltWithoutVectors,
    /// The sidecar was built for another model, embedder or chunker.
    ModelMismatch,
    /// The manifest records no per-way hashes to check against.
    NoHashes,
    /// Enabled ways the alias corpus holds but the sidecar lacks, or holds at
    /// another content hash.
    Incomplete { missing: Vec<String>, stale: Vec<String> },
}

impl std::fmt::Display for Fallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let list = |ids: &[String]| {
            let shown: Vec<&str> = ids.iter().take(3).map(String::as_str).collect();
            let more = ids.len().saturating_sub(3);
            if more > 0 { format!("{} and {more} more", shown.join(", ")) } else { shown.join(", ") }
        };
        match self {
            Fallback::NoEmbedder => write!(f, "way-embed not installed"),
            Fallback::Absent => write!(f, "absent; run `ways corpus`"),
            Fallback::BuildFailed(why) => write!(f, "build failed: {why}"),
            Fallback::NoVectors => write!(f, "the installed way-embed cannot return chunk vectors, which way-embed 1.2.0 adds; `ways update` installs it once it is published"),
            Fallback::BuiltWithoutVectors => write!(f, "built without chunk vectors; run `ways corpus`"),
            Fallback::ModelMismatch => write!(f, "built for another model, way-embed or chunker; run `ways corpus`"),
            Fallback::NoHashes => write!(f, "the manifest has no way hashes; run `ways corpus`"),
            Fallback::Incomplete { missing, stale } => {
                let mut parts = Vec::new();
                if !missing.is_empty() {
                    parts.push(format!("missing {}", list(missing)));
                }
                if !stale.is_empty() {
                    parts.push(format!("stale {}", list(stale)));
                }
                write!(f, "incomplete ({}); run `ways corpus`", parts.join("; "))
            }
        }
    }
}

impl Sidecar {
    /// Ways with a record.
    pub(crate) fn way_count(&self) -> usize {
        self.index.len()
    }

    /// Section vectors held.
    pub(crate) fn vector_count(&self) -> usize {
        self.vectors.len().checked_div(self.dim).unwrap_or(0)
    }

    /// ADR-701 §7: `Ok` when the sidecar was built with `model` and every way
    /// in `enabled` that the alias corpus holds (`alias`, the manifest's
    /// `way_hashes`) has a record at the same content hash. An enabled way the
    /// alias corpus lacks cannot win a chunk, so it does not count.
    pub(crate) fn check<'a>(
        &self,
        model: &str,
        enabled: impl IntoIterator<Item = &'a str>,
        alias: &HashMap<String, u64>,
    ) -> Result<(), Fallback> {
        if self.model != model {
            return Err(Fallback::ModelMismatch);
        }
        if alias.is_empty() {
            return Err(Fallback::NoHashes);
        }
        let (mut missing, mut stale) = (Vec::new(), Vec::new());
        for id in enabled {
            let Some(want) = alias.get(id) else { continue };
            match self.index.get(id) {
                None => missing.push(id.to_string()),
                Some((h, _, _)) if h != want => stale.push(id.to_string()),
                Some(_) => {}
            }
        }
        if missing.is_empty() && stale.is_empty() {
            Ok(())
        } else {
            missing.sort();
            stale.sort();
            Err(Fallback::Incomplete { missing, stale })
        }
    }

    /// Max cosine of `v` over the way's section vectors. `None` when the way
    /// has no sections (or no record), so the caller falls back to its alias.
    pub(crate) fn max_cosine(&self, id: &str, v: &[f32]) -> Option<f64> {
        let &(_, row, n) = self.index.get(id)?;
        if n == 0 || self.dim == 0 || v.len() != self.dim {
            return None;
        }
        self.vectors[row * self.dim..(row + n) * self.dim]
            .chunks_exact(self.dim)
            .map(|s| dot(s, v))
            .reduce(f64::max)
    }
}

/// Dot product in f64. Vectors from way-embed are L2-normalised, so this is
/// the cosine.
pub(crate) fn dot(a: &[f32], b: &[f32]) -> f64 {
    a.iter().zip(b).map(|(x, y)| *x as f64 * *y as f64).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(v: &[f32]) -> Vec<f32> {
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter().map(|x| x / n).collect()
    }

    fn sample() -> Vec<WaySections> {
        vec![
            WaySections { id: "a/one".into(), hash: 0x1111, vectors: vec![unit(&[1.0, 0.0, 0.0]), unit(&[0.0, 1.0, 0.0])] },
            WaySections { id: "b".into(), hash: 0x2222, vectors: vec![] },
            WaySections { id: "c/three".into(), hash: 0x3333, vectors: vec![unit(&[1.0, 1.0, 1.0])] },
        ]
    }

    #[test]
    fn round_trip_keeps_model_dim_hashes_and_vectors() {
        let bytes = encode("minilm-l6-v2.gguf:21717952", 3, &sample()).unwrap();
        let sc = decode(&bytes).unwrap();
        assert_eq!(sc.model, "minilm-l6-v2.gguf:21717952");
        assert_eq!(sc.dim, 3);
        assert_eq!(sc.index["a/one"], (0x1111, 0, 2));
        assert_eq!(sc.index["b"], (0x2222, 2, 0));
        assert_eq!(sc.index["c/three"], (0x3333, 2, 1));
        assert_eq!(sc.vectors.len(), 9);
        assert_eq!(&sc.vectors[6..9], unit(&[1.0, 1.0, 1.0]).as_slice());
    }

    #[test]
    fn round_trip_through_a_file() {
        let dir = std::env::temp_dir().join(format!("ways-sidecar-rt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        write(&path, &encode("m", 3, &sample()).unwrap()).unwrap();
        let sc = read(&path).unwrap();
        assert_eq!(sc.index["c/three"].0, 0x3333);
        let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from(FILE)], "no staging file left");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_truncated_or_foreign_file_reads_as_absent() {
        let bytes = encode("m", 3, &sample()).unwrap();
        assert!(decode(&bytes[..bytes.len() - 1]).is_none());
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(decode(&extra).is_none());
        let mut v2 = bytes.clone();
        v2[8] = 2;
        assert!(decode(&v2).is_none(), "another version");
        assert!(decode(b"not a sidecar at all").is_none());
    }

    /// A corrupt header must read as absent, never allocate what it claims.
    /// way_count is the u32 after magic, version and dim.
    #[test]
    fn a_huge_way_count_reads_as_absent() {
        let mut bytes = encode("m", 3, &sample()).unwrap();
        bytes[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&bytes).is_none());
        let mut bytes = encode("m", 3, &sample()).unwrap();
        bytes[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
        bytes[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&bytes).is_none(), "huge dim and vector count");
    }

    /// Fuzz-style: every truncation and many random byte mutations decode to
    /// `None` or a sidecar, never a panic or an abort. A mutation inside the
    /// vector payload is still a valid file, so only the header region is
    /// required to be rejected when it changes a length.
    #[test]
    fn decode_never_panics_on_damaged_input() {
        let bytes = encode("minilm-l6-v2.gguf:5", 3, &sample()).unwrap();
        for n in 0..bytes.len() {
            assert!(decode(&bytes[..n]).is_none(), "truncated to {n}");
        }
        // xorshift, fixed seed: reproducible.
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..20_000 {
            let mut b = bytes.clone();
            for _ in 0..(1 + next() % 4) {
                let i = (next() % b.len() as u64) as usize;
                b[i] = next() as u8;
            }
            let _ = decode(&b);
        }
    }

    #[test]
    fn encode_refuses_a_vector_of_the_wrong_dimension() {
        let ways = vec![WaySections { id: "x".into(), hash: 1, vectors: vec![vec![1.0, 0.0]] }];
        assert!(encode("m", 3, &ways).is_none());
    }

    fn alias() -> HashMap<String, u64> {
        [("a/one", 0x1111), ("b", 0x2222), ("c/three", 0x3333)].iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn covers_every_enabled_way_at_the_alias_hashes() {
        let sc = decode(&encode("m", 3, &sample()).unwrap()).unwrap();
        assert!(sc.check("m", ["a/one", "b", "c/three"], &alias()).is_ok());
        assert!(sc.check("m", ["a/one"], &alias()).is_ok(), "a disabled way need not be enabled");
    }

    #[test]
    fn incomplete_when_an_enabled_way_is_missing() {
        let mut ways = sample();
        ways.remove(2);
        let sc = decode(&encode("m", 3, &ways).unwrap()).unwrap();
        assert_eq!(sc.check("m", ["a/one", "b", "c/three"], &alias()), Err(Fallback::Incomplete { missing: vec!["c/three".into()], stale: vec![] }));
        // The missing way is disabled: the rest is complete.
        assert!(sc.check("m", ["a/one", "b"], &alias()).is_ok());
    }

    /// An enabled way the alias corpus does not hold (a worktree's or an
    /// unregistered project's way) cannot win a chunk, so it does not make the
    /// sidecar incomplete.
    #[test]
    fn an_enabled_way_absent_from_the_alias_corpus_does_not_disable_the_sidecar() {
        let sc = decode(&encode("m", 3, &sample()).unwrap()).unwrap();
        assert!(sc.check("m", ["a/one", "b", "-home-me-proj/local/way"], &alias()).is_ok());
    }

    #[test]
    fn incomplete_when_an_enabled_way_is_stale() {
        let mut ways = sample();
        ways[0].hash = 0xdead;
        let sc = decode(&encode("m", 3, &ways).unwrap()).unwrap();
        assert_eq!(sc.check("m", ["a/one", "b"], &alias()), Err(Fallback::Incomplete { missing: vec![], stale: vec!["a/one".into()] }));
    }

    #[test]
    fn incomplete_when_the_model_changed_or_the_manifest_has_no_hashes() {
        let sc = decode(&encode("m", 3, &sample()).unwrap()).unwrap();
        assert_eq!(sc.check("other", ["a/one"], &alias()), Err(Fallback::ModelMismatch));
        assert_eq!(sc.check("m", ["a/one"], &HashMap::new()), Err(Fallback::NoHashes));
    }

    #[test]
    fn max_cosine_is_the_best_section() {
        let sc = decode(&encode("m", 3, &sample()).unwrap()).unwrap();
        let v = unit(&[0.6, 0.8, 0.0]);
        // Hand-computed: sections (1,0,0) and (0,1,0) give 0.6 and 0.8.
        assert!((sc.max_cosine("a/one", &v).unwrap() - 0.8).abs() < 1e-6);
        assert!(sc.max_cosine("b", &v).is_none(), "no sections");
        assert!(sc.max_cosine("nope", &v).is_none(), "no record");
    }

    #[test]
    fn the_manifest_view_reads_hashes_vectors_and_reason() {
        let dir = std::env::temp_dir().join(format!("ways-sidecar-mf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let m = dir.join("embed-manifest.json");
        std::fs::write(&m, r#"{"way_hashes":{"a/one":"0000000000001111","bad":"zz"},"body_sidecar":{"vectors":true}}"#).unwrap();
        let v = ManifestView::read(&m);
        assert_eq!(v.alias.get("a/one"), Some(&0x1111));
        assert!(!v.alias.contains_key("bad"));
        assert!(v.vectors);
        assert!(v.reason.is_none());
        std::fs::write(&m, r#"{"global_hash":"x","body_sidecar":{"file":null,"reason":"boom"}}"#).unwrap();
        let v = ManifestView::read(&m);
        assert!(v.alias.is_empty());
        assert!(!v.vectors);
        assert_eq!(v.reason.as_deref(), Some("boom"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A zero-dimension sidecar that still claims sections (a damaged or
    /// degenerate build) scores nothing rather than panicking.
    #[test]
    fn max_cosine_on_a_zero_dimension_sidecar_is_none() {
        let ways = vec![WaySections { id: "z".into(), hash: 1, vectors: vec![vec![], vec![]] }];
        let sc = decode(&encode("m", 0, &ways).unwrap()).unwrap();
        assert!(sc.max_cosine("z", &[]).is_none());
    }

    /// The model id moves with the way-embed binary and names the chunker.
    #[test]
    fn model_id_changes_with_the_embedder_and_carries_the_chunker_revision() {
        let dir = std::env::temp_dir().join(format!("ways-sidecar-mid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(crate::paths::EN_MODEL), "model").unwrap();
        std::fs::write(dir.join("old"), "way-embed 1.1.2").unwrap();
        std::fs::write(dir.join("new"), "way-embed 1.2.0 longer").unwrap();
        let a = model_id(&dir, &dir.join("old")).unwrap();
        let b = model_id(&dir, &dir.join("new")).unwrap();
        assert_ne!(a, b);
        assert!(a.starts_with("minilm-l6-v2.gguf:5|way-embed:15:"), "{a}");
        assert!(a.ends_with(&format!("|sections:{CHUNKER_REV}")), "{a}");
        assert!(model_id(&dir, &dir.join("absent")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The sidecar replaces its path: a write never goes through a link, and
    /// a write that cannot stage leaves the old bytes.
    #[cfg(unix)]
    #[test]
    fn write_replaces_the_sidecar_atomically() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("ways-sidecar-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("target.bin");
        std::fs::write(&target, "old").unwrap();
        let path = dir.join(FILE);
        std::os::unix::fs::symlink(&target, &path).unwrap();
        write(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"old", "written through the link");
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 2, "staging left behind: {names:?}");

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let can_stage = std::fs::File::create(dir.join("probe")).is_ok();
        let res = write(&path, b"newer");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        if can_stage {
            eprintln!("SKIPPED write_replaces_the_sidecar_atomically (read-only half): directory modes do not bind this user (root?)");
        } else {
            assert!(res.is_err());
            assert_eq!(std::fs::read(&path).unwrap(), b"new");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
