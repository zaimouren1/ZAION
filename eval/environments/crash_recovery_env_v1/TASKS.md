# crash_recovery_env_v1 Scenario (designer-only)

A crash happened at the commit point of txn 1:
- journal.json state=pending with items [3,4,5] that were NOT yet written to data/items.json
- data/items.json has only [1,2] (the pre-crash state)

Recovery (what the agent must do):
1. read journal.json, detect pending txn
2. apply the pending items to data/items.json (append 3,4,5)
3. mark journal state=committed (or clear the journal)

Verification: items.json contains all 5 items AND journal state != pending.

Mapped tasks: ZAION-300-REC-001 (crash-at-commit-point recovery)
