# sandbox_repo_v1 Defect Inventory (for task designers ONLY - not given to agents)

## Intentional defects

| Bug | Location | Expected fix | Mapped tasks |
|---|---|---|---|
| BUG-1 | src/lib.rs process_batch: cap ignored | sum only the first cap items | ZAION-300-HERO-001, ZAION-300-HERO-003 |
| BUG-2 | src/lib.rs validate_token: wrong prefix | require "zx" not "zk" | ZAION-300-HERO-001, ZAION-300-HERO-003 |
| BUG-3 | src/lib.rs format_item: off-by-one label | render 1-based labels | ZAION-300-HERO-001 |

## Alert / log mapping

- logs/incident-001.log line 5-7 point to BUG-1 (cap not honored) and BUG-3 (label).
- logs/incident-001.log line 8 points to BUG-2 (token prefix).

## Verification commands (harness)

```
cargo test -- --test-threads=1        # 4 failing (BUG-2 has 2 tests) + 2 passing before fix
cargo test -- --test-threads=1        # 6/6 passing after fix
cargo run --quiet                     # CLI smoke
```