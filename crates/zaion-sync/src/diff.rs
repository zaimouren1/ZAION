/// Compute the symmetric difference between two sets of event IDs.
///
/// This is a pure function — no ledger or I/O involved — which makes it
/// easy to test and reuse in both CLI and relay-sync scenarios.
#[derive(Debug, Clone)]
pub struct SyncDiff {
    /// Number of event IDs in the local set.
    pub local_count: usize,
    /// Number of event IDs in the remote set.
    pub remote_count: usize,
    /// Event IDs that are present in `remote` but absent from `local`.
    pub missing_locally: Vec<String>,
    /// Event IDs that are present in `local` but absent from `remote`.
    pub missing_remotely: Vec<String>,
}

impl SyncDiff {
    /// Compute what events each side is missing.
    ///
    /// Order of `missing_locally` and `missing_remotely` matches the order
    /// in which the IDs appear in the input slices.
    pub fn compute(local_ids: &[String], remote_ids: &[String]) -> Self {
        use std::collections::HashSet;

        let local_set: HashSet<&str> = local_ids.iter().map(|s| s.as_str()).collect();
        let remote_set: HashSet<&str> = remote_ids.iter().map(|s| s.as_str()).collect();

        let missing_locally: Vec<String> = remote_ids
            .iter()
            .filter(|id| !local_set.contains(id.as_str()))
            .cloned()
            .collect();

        let missing_remotely: Vec<String> = local_ids
            .iter()
            .filter(|id| !remote_set.contains(id.as_str()))
            .cloned()
            .collect();

        Self {
            local_count: local_ids.len(),
            remote_count: remote_ids.len(),
            missing_locally,
            missing_remotely,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn diff_detects_missing_events() {
        let local = ids(&["evt-1", "evt-2"]);
        let remote = ids(&["evt-2", "evt-3", "evt-4"]);
        let diff = SyncDiff::compute(&local, &remote);

        assert_eq!(diff.local_count, 2);
        assert_eq!(diff.remote_count, 3);
        assert_eq!(diff.missing_locally, ids(&["evt-3", "evt-4"]));
        assert_eq!(diff.missing_remotely, ids(&["evt-1"]));
    }

    #[test]
    fn diff_symmetric_empty_when_identical() {
        let both = ids(&["evt-a", "evt-b", "evt-c"]);
        let diff = SyncDiff::compute(&both, &both);

        assert!(diff.missing_locally.is_empty());
        assert!(diff.missing_remotely.is_empty());
        assert_eq!(diff.local_count, 3);
        assert_eq!(diff.remote_count, 3);
    }

    #[test]
    fn diff_all_missing_locally_when_local_empty() {
        let local = ids(&[]);
        let remote = ids(&["evt-1", "evt-2"]);
        let diff = SyncDiff::compute(&local, &remote);

        assert_eq!(diff.missing_locally.len(), 2);
        assert!(diff.missing_remotely.is_empty());
    }

    #[test]
    fn diff_all_missing_remotely_when_remote_empty() {
        let local = ids(&["evt-x", "evt-y"]);
        let remote = ids(&[]);
        let diff = SyncDiff::compute(&local, &remote);

        assert!(diff.missing_locally.is_empty());
        assert_eq!(diff.missing_remotely.len(), 2);
    }
}
