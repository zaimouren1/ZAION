# Zaion Product Evaluation

This directory contains executable product-gate contracts. It is separate from
historical comparison ledgers and from implementation plans.

## Files

- `benchmark_manifest.schema.json`: JSON Schema for benchmark manifests.
- `benchmarks/zaion_300_v1.json`: M0 benchmark scaffold pinned to a Hermes
  source commit.
- Future `fixtures/`: deterministic task inputs.
- Future `evidence/`: command output, reports, screenshots, and packaged
  artifact results referenced by verified tasks.

## What "300" Means

`zaion_300_v1` reserves 300 task slots across 15 product categories. The M0
manifest contains one task-family record per category with a `slots` count.
All families are `planned`, all scores are null, all evidence arrays are
empty, and `claimed_verified_slots` is zero.

These are allocation slots, not 300 fabricated executions. Before a family can
be scored, replace or expand its slots into executable task records with
fixtures, commands, expected outcomes, and retained evidence. A record may be
`verified` only when its result covers every slot represented by that record.

## Task Lifecycle

| Status | Meaning |
| --- | --- |
| `planned` | Acceptance is defined; executable case is not ready |
| `ready` | Fixture and command exist; no passing result claimed |
| `running` | An evaluation run is active |
| `verified` | All represented slots passed and evidence is retained |
| `blocked` | A named external or product blocker prevents execution |
| `retired` | Replaced by a versioned task with migration rationale |

Source inspection can move a task toward `ready`; it cannot produce a parity,
surpass, or 10/10 result by itself.

## Evidence Contract

A verified task requires:

- a unique evidence ID;
- evidence kind;
- an existing repository-relative or absolute artifact path;
- observed result `pass`;
- observation timestamp;
- a result object whose `verified_slots` equals the task's `slots`;
- a numeric score and evidence grade.

A score of 10 additionally requires task status `verified`, non-empty evidence,
all parity/surpass/10 acceptance criteria, and
`evidence_grade = release_verified`.

## Validate

From the repository root:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/validate-product-gates.ps1
```

The validator is read-only. It checks:

- required governance assets and valid JSON;
- category and scorecard weights total exactly 100;
- risk-adjusted mission weights remain fixed at `40/20/15/15/10` and total 100;
- category IDs and weights agree between Markdown and JSON;
- Hermes baseline has a 40-character commit;
- category slots and task slots both total 300;
- every task has `id`, `category`, `status`, `source`, and
  `acceptance`;
- verified-slot claims equal evidence-backed results;
- unverified work cannot claim 10/10.

The JSON Schema supports editor and external validator integration. The
PowerShell script enforces cross-field rules such as sums and evidence gates
that JSON Schema cannot express portably.

## Versioning

Do not rewrite completed evidence in place. Create a new benchmark version when
the competitor commit, weights, task semantics, or scoring policy changes.
Record the old-to-new mapping in the new manifest or an adjacent migration
note.
