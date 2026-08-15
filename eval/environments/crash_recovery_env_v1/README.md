# crash_recovery_env_v1

Benchmark scenario: crash at a commit point with a pending journal.
- data/items.json: pre-crash committed state [1,2]
- journal.json: pending txn with items [3,4,5]

The agent must recover: apply journal to data, mark committed.
See TASKS.md for the inventory (designer-only).
