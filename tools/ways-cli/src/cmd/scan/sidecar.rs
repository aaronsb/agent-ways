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
//! corpus way's content hash. A scan uses the sidecar only when its model id is
//! the installed model's and every enabled way has a record whose hash equals
//! the manifest's. Anything less and the scan behaves exactly as without it.

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

/// The id of the English model in `engine_dir`: file name and size, so a
/// replaced model invalidates the sidecar.
pub(crate) fn model_id(engine_dir: &Path) -> Option<String> {
    let len = engine_dir.join(crate::paths::EN_MODEL).metadata().ok()?.len();
    Some(format!("{}:{len}", crate::paths::EN_MODEL))
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
    let vectors = r.bytes[r.at..]
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
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
    let tmp = path.with_extension(format!("bin.{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Read and parse the sidecar at `path`.
pub(crate) fn read(path: &Path) -> Option<Sidecar> {
    decode(&std::fs::read(path).ok()?)
}

/// The manifest's `way_hashes`: each alias corpus way's content hash. Empty
/// when the manifest predates the field.
pub(crate) fn alias_hashes(manifest: &Path) -> HashMap<String, u64> {
    std::fs::read_to_string(manifest)
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .map(|m| alias_hashes_from(&m))
        .unwrap_or_default()
}

/// [`alias_hashes`] of a parsed manifest.
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

impl Sidecar {
    /// ADR-701 §7: true when the sidecar was built with `model` and every id in
    /// `enabled` has a record at the alias corpus's content hash. A way missing
    /// from either side, or recorded at another hash, makes it incomplete.
    pub(crate) fn covers<'a>(
        &self,
        model: &str,
        enabled: impl IntoIterator<Item = &'a str>,
        alias: &HashMap<String, u64>,
    ) -> bool {
        self.model == model
            && enabled.into_iter().all(|id| {
                matches!((alias.get(id), self.index.get(id)), (Some(a), Some((h, _, _))) if a == h)
            })
    }

    /// Max cosine of `v` over the way's section vectors. `None` when the way
    /// has no sections (or no record), so the caller falls back to its alias.
    pub(crate) fn max_cosine(&self, id: &str, v: &[f32]) -> Option<f64> {
        let &(_, row, n) = self.index.get(id)?;
        if n == 0 || v.len() != self.dim {
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

/// The alias vector of corpus id `id`, read from the alias corpus at `corpus`.
/// Only the one line is parsed: this runs for a sectionless survivor, rarely.
pub(crate) fn alias_vector(corpus: &Path, id: &str) -> Option<Vec<f32>> {
    let text = std::fs::read_to_string(corpus).ok()?;
    let key = format!("{{\"id\":{}", serde_json::to_string(id).ok()?);
    let line = text.lines().find(|l| l.starts_with(&key))?;
    let row: serde_json::Value = serde_json::from_str(line).ok()?;
    row.get("embedding")?.as_array()?.iter().map(|x| x.as_f64().map(|f| f as f32)).collect()
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
        assert!(sc.covers("m", ["a/one", "b", "c/three"], &alias()));
        assert!(sc.covers("m", ["a/one"], &alias()), "a disabled way need not be enabled");
    }

    #[test]
    fn incomplete_when_an_enabled_way_is_missing() {
        let mut ways = sample();
        ways.remove(2);
        let sc = decode(&encode("m", 3, &ways).unwrap()).unwrap();
        assert!(!sc.covers("m", ["a/one", "b", "c/three"], &alias()));
        // The missing way is disabled: the rest is complete.
        assert!(sc.covers("m", ["a/one", "b"], &alias()));
    }

    #[test]
    fn incomplete_when_an_enabled_way_is_stale() {
        let mut ways = sample();
        ways[0].hash = 0xdead;
        let sc = decode(&encode("m", 3, &ways).unwrap()).unwrap();
        assert!(!sc.covers("m", ["a/one", "b"], &alias()));
    }

    #[test]
    fn incomplete_when_the_model_changed_or_the_manifest_has_no_hashes() {
        let sc = decode(&encode("m", 3, &sample()).unwrap()).unwrap();
        assert!(!sc.covers("other", ["a/one"], &alias()));
        assert!(!sc.covers("m", ["a/one"], &HashMap::new()));
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
    fn alias_hashes_reads_the_manifest_field() {
        let dir = std::env::temp_dir().join(format!("ways-sidecar-mf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let m = dir.join("embed-manifest.json");
        std::fs::write(&m, r#"{"way_hashes":{"a/one":"0000000000001111","bad":"zz"}}"#).unwrap();
        let h = alias_hashes(&m);
        assert_eq!(h.get("a/one"), Some(&0x1111));
        assert!(!h.contains_key("bad"));
        std::fs::write(&m, r#"{"global_hash":"x"}"#).unwrap();
        assert!(alias_hashes(&m).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn alias_vector_reads_one_line() {
        let dir = std::env::temp_dir().join(format!("ways-sidecar-av-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let c = dir.join("ways-corpus-en.jsonl");
        std::fs::write(
            &c,
            "{\"id\":\"a/one-more\",\"embedding\":[0.0,1.0]}\n{\"id\":\"a/one\",\"description\":\"d\",\"embedding\":[0.5,0.25]}\n",
        )
        .unwrap();
        assert_eq!(alias_vector(&c, "a/one"), Some(vec![0.5, 0.25]));
        assert_eq!(alias_vector(&c, "zzz"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
