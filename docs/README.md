# Zaion Documentation Index

Use this page to find the current source of truth. Documents not listed as
current may still contain valuable design history, but should not override the
code, tests, `AGENTS.md`, or the dated project status.

## Start here

- [Project map](PROJECT_MAP.md): repository layout, crate responsibilities,
  product entry points, and change routing.
- [Project status](PROJECT_STATUS.md): dated health snapshot, blockers, and
  cleanup stages.
- [Active roadmap](../ROADMAP.md): current priorities and acceptance gates.
- [Quick start](QUICK_START.md): first local run.
- [Capability status](CAPABILITY_STATUS.md): stable, beta, and experimental
  product surfaces.
- [CLI stability baseline](CLI_STABILITY.md): command compatibility contract.
- [Doctor troubleshooting](DOCTOR.md): local health and configuration checks.
- [Provider setup](PROVIDERS.md): provider keys and endpoints.

## Architecture and runtime

- [Gateway](GATEWAY.md)
- [Memory system](MEMORY_SYSTEM.md)
- [Self-healing / Ouroboros](SELF_HEALING.md)
- [Proactive behavior](PROACTIVE_BEHAVIOR.md)
- [TUI architecture](TUI_ARCHITECTURE.md)
- [TUI v2 architecture](TUI_V2_ARCHITECTURE.md)
- [Streaming renderer](STREAMING_RENDERER.md)
- [Agentic panel](AGENTIC_PANEL.md)
- [Phase 8 runtime proof](PHASE8.md)

## Product governance and evaluation

- [Product scorecard](PRODUCT_SCORECARD.md): weighted parity, surpass, and
  10/10 evidence rules; currently unscored.
- [Product threat model](THREAT_MODEL.md): trust boundaries, release-blocking
  threats, and security invariants.
- [Product evaluation harness](../eval/README.md): versioned benchmark
  manifests, evidence lifecycle, and read-only validation.

## Release and operations

- [Release chain](RELEASE.md)
- [Integration status](INTEGRATION_STATUS.md)
- [Execution tracker](EXECUTION_TRACKER.md)
- [Feature audit, 2026-06-03](FEATURE_AUDIT_2026_06_03.md)

## Comparative research

- [Zaion vs latest Hermes](zaion_vs_hermes.md): current comparison report;
  overall label remains `PARTIAL`.
- [Zaion vs OpenClaw](zaion_vs_openclaw.md): older comparison context.
- [Claude Code TUI patterns](CLAUDE_CODE_TUI_PATTERNS.md): implementation
  research, not a Zaion product contract.
- [Claude Code TUI replication plan](CLAUDE_CODE_TUI_REPLICATION_PLAN.md):
  historical design plan.

## Historical completion reports and blueprints

Files named `*_COMPLETE.md`, `ROADMAP_*`, documents under `docs/blueprints/`,
and dated plans/specs under `docs/superpowers/` are retained as evidence and
design history. A completion title records the scope of that historical stage;
it is not proof that the current workspace still satisfies every claim.

The standalone public website was intentionally retired on 2026-07-13. Its
design documents are retained as historical evidence:

- [Website architecture](archive/website/ZAION_WEBSITE_ARCHITECTURE.md)
- [Website plan](archive/website/ZAION_WEBSITE_PLAN.md)

## Documentation rules

1. Current product facts belong in `README.md`, `PROJECT_MAP.md`,
   `PROJECT_STATUS.md`, `CAPABILITY_STATUS.md`, or `CLI_STABILITY.md`.
2. Plans must use `SURPASSED`, `PARTIAL`, and `OPEN` only with source and
   verification evidence.
3. Add new progress entries at the top of the three active ledgers; do not
   silently rewrite historical evidence.
4. Repair corrupted historical text only from an intact source or commit.
5. Keep the root and documentation `AGENTS.md` files synchronized when the
   execution baseline changes.
