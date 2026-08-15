# Zaion v5.0 Integration Status

Generated: 2026-05-01

This is a current-state integration note. The v5.0 systems are present in the
workspace, but presence is not the same as product maturity.

## Current Evidence

| System | Crate | Current maturity |
| --- | --- | --- |
| Programmable Ego-Matrix | `zaion-ego` | Experimental |
| Zero-Token Autonomic | `zaion-autonomic` | Experimental |
| Hardware Proprioception | `zaion-proprioception` | Beta/experimental |
| Metabolic Engine | `zaion-metabolic` | Beta |
| Entropic Curiosity | `zaion-curiosity` | Experimental |

## Boundary

These systems may have unit tests and CLI surfaces, but they must not be
described as stable until they are wired into the runtime with:

- module-specific `status` or `doctor` output,
- user-path integration tests,
- documented safety boundaries,
- proof commands in `zaion macro verify`,
- no placeholder security or autonomy claims.

## Next Work

1. Keep the stable first path small and regression-tested.
2. Promote each v5.0 system through beta only after doctor/status and docs are
   in place.
3. Promote to stable only after recovery behavior, CI coverage, and security
   boundaries are proven.
