# sandbox_repo_v1

Benchmark sandbox repository for Zaion 300-task eval (dev/SRE hero mission).

- Rust crate with three deliberate defects (failing tests).
- Service log with error patterns pointing at root causes.
- Config with a cap the code ignores.

See TASKS.md for the defect inventory (designer-only). Build:

```
cargo test -- --test-threads=1
```
