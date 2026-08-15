/// HNSW-backed approximate nearest-neighbor index.
///
/// Uses `instant-distance` (pure Rust, no C deps) for fast ANN search.
/// The index is rebuilt lazily when new vectors are added and a search is
/// subsequently performed, so write-heavy workloads pay the rebuild cost
/// once per batch rather than on every insert.
use instant_distance::{Builder, HnswMap, Search};

use crate::MemoryError;

// ── internal point wrapper ────────────────────────────────────────────────────

/// Newtype so we can implement `instant_distance::Point` for a `Vec<f32>`.
#[derive(Clone)]
struct F32Vec(Vec<f32>);

impl instant_distance::Point for F32Vec {
    fn distance(&self, other: &Self) -> f32 {
        cosine_distance(&self.0, &other.0)
    }
}

// ── public interface ──────────────────────────────────────────────────────────

/// An HNSW index mapping opaque `u64` IDs → f32 embedding vectors.
///
/// Insertions are buffered; the underlying HNSW graph is rebuilt lazily on
/// the first `search` call after one or more `add` calls.  This is efficient
/// for RAG workloads where vectors are ingested in batches and queries are
/// much more frequent than writes.
pub struct HnswIndex {
    /// Raw storage: (external_id, embedding).
    entries: Vec<(u64, Vec<f32>)>,
    /// Compiled HNSW map (None = needs rebuild or empty).
    compiled: Option<HnswMap<F32Vec, u64>>,
    /// Whether `entries` has changed since the last build.
    dirty: bool,
}

impl HnswIndex {
    /// Create a new empty index.
    #[inline]
    pub fn new(_dims: usize) -> Self {
        Self {
            entries: Vec::new(),
            compiled: None,
            dirty: false,
        }
    }

    /// Add a vector to the index.  The change is buffered; the HNSW graph is
    /// rebuilt on the next `search` call.
    pub fn add(&mut self, id: u64, vector: &[f32]) -> Result<(), MemoryError> {
        if vector.is_empty() {
            return Err(MemoryError::Other("empty vector".into()));
        }
        self.entries.push((id, vector.to_vec()));
        self.dirty = true;
        Ok(())
    }

    /// Search for the `k` approximate nearest neighbours of `query`.
    ///
    /// Returns `(id, distance)` pairs sorted by distance ascending.
    /// An empty result is returned when the index is empty.
    pub fn search(&mut self, query: &[f32], k: usize) -> Vec<(u64, f32)> {
        if self.entries.is_empty() {
            return Vec::new();
        }
        if self.dirty {
            self.rebuild();
        }
        let Some(map) = &self.compiled else {
            return brute_force_search(&self.entries, query, k);
        };

        let q = F32Vec(query.to_vec());
        let mut search = Search::default();
        map.search(&q, &mut search)
            .take(k)
            .map(|item| (*item.value, item.distance))
            .collect()
    }

    /// Total number of indexed vectors.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the index contains no vectors.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    // ── private helpers ───────────────────────────────────────────────────────

    /// Rebuild the HNSW graph from the current `entries` buffer.
    /// `instant-distance` requires at least 1 point.
    fn rebuild(&mut self) {
        let n = self.entries.len();
        if n == 0 {
            self.compiled = None;
            self.dirty = false;
            return;
        }

        let points: Vec<F32Vec> = self
            .entries
            .iter()
            .map(|(_, v)| F32Vec(v.clone()))
            .collect();
        let values: Vec<u64> = self.entries.iter().map(|(id, _)| *id).collect();

        let map = Builder::default().build(points, values);
        self.compiled = Some(map);
        self.dirty = false;
    }
}

// ── fallback / helpers ────────────────────────────────────────────────────────

/// Brute-force fallback for very small sets (used when the HNSW failed to compile).
fn brute_force_search(entries: &[(u64, Vec<f32>)], query: &[f32], k: usize) -> Vec<(u64, f32)> {
    let mut scored: Vec<(u64, f32)> = entries
        .iter()
        .map(|(id, v)| (*id, cosine_distance(query, v)))
        .collect();
    scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(k);
    scored
}

/// Cosine distance in [0, 2].  Lower = more similar.
fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    let (mut dot, mut na, mut nb) = (0f32, 0f32, 0f32);
    for i in 0..len {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom < 1e-10 {
        return 2.0;
    }
    1.0 - (dot / denom)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn random_vec(dims: usize, seed: u64) -> Vec<f32> {
        // simple LCG for reproducibility without extra deps
        let mut state = seed.wrapping_add(1);
        (0..dims)
            .map(|_| {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                // map to [-1, 1]
                ((state >> 33) as f32) / (u32::MAX as f32) * 2.0 - 1.0
            })
            .collect()
    }

    #[test]
    fn test_add_and_search_basic() {
        let mut idx = HnswIndex::new(4);
        idx.add(1, &[1.0, 0.0, 0.0, 0.0]).unwrap();
        idx.add(2, &[0.0, 1.0, 0.0, 0.0]).unwrap();
        idx.add(3, &[0.0, 0.0, 1.0, 0.0]).unwrap();

        let results = idx.search(&[1.0, 0.0, 0.0, 0.0], 1);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 1, "closest to e1 should be id=1");
        assert!(results[0].1 < 1e-4, "distance to exact match should be ~0");
    }

    #[test]
    fn test_empty_index() {
        let mut idx = HnswIndex::new(8);
        let results = idx.search(&[1.0; 8], 5);
        assert!(results.is_empty());
    }

    #[test]
    fn test_len_and_is_empty() {
        let mut idx = HnswIndex::new(4);
        assert!(idx.is_empty());
        idx.add(10, &[0.1, 0.2, 0.3, 0.4]).unwrap();
        assert_eq!(idx.len(), 1);
        assert!(!idx.is_empty());
    }

    #[test]
    fn test_error_on_empty_vector() {
        let mut idx = HnswIndex::new(4);
        let result = idx.add(1, &[]);
        assert!(result.is_err());
    }

    /// Performance smoke-test: 1 000 vectors × 64 dims, top-5 ANN query on a
    /// pre-built index must complete in under 100 ms.
    ///
    /// The HNSW graph is built once (warm-up search), then the actual timed
    /// query runs against the already-compiled index.  Build time is excluded
    /// because in production the index is built once and queried many times.
    #[test]
    fn test_search_performance_1k_vectors() {
        const N: usize = 1_000;
        const DIMS: usize = 64;

        let mut idx = HnswIndex::new(DIMS);
        for i in 0..N {
            let v = random_vec(DIMS, i as u64);
            idx.add(i as u64, &v).unwrap();
        }

        // Warm-up: trigger the one-time graph build (not measured).
        let query = random_vec(DIMS, 999_999);
        let _ = idx.search(&query, 5);

        // Timed query against the pre-built index.
        let query2 = random_vec(DIMS, 123_456);
        let start = std::time::Instant::now();
        let results = idx.search(&query2, 5);
        let elapsed = start.elapsed();

        assert_eq!(results.len(), 5, "should return 5 neighbours");
        assert!(
            elapsed.as_millis() < 100,
            "ANN query took {}ms, want <100ms",
            elapsed.as_millis()
        );
    }
}
