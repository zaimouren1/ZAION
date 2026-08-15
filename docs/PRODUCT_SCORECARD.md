# Zaion Product Scorecard

Status: evaluation scaffold, no current product score

This scorecard defines how Zaion is compared with the pinned Hermes source
baseline and how a future product-readiness claim must be earned. It is not a
completion report. The machine-readable source is
`eval/benchmarks/zaion_300_v1.json`; this document explains the policy.

Hermes baseline:
`main@9c0807070388c4f612a827230f1314ebbf24e857`
(`2026-05-24 15:57:26 -0700`). A newer baseline may replace it only when the
manifest records the new commit and the affected tasks are recalibrated.

## Scorecard

| Category ID | Weight | Current score | Evidence state |
| --- | ---: | ---: | --- |
| `onboarding` | 7 | `UNSCORED` | `NOT_EVALUATED` |
| `tui` | 8 | `UNSCORED` | `NOT_EVALUATED` |
| `session` | 8 | `UNSCORED` | `NOT_EVALUATED` |
| `tools` | 10 | `UNSCORED` | `NOT_EVALUATED` |
| `skills` | 5 | `UNSCORED` | `NOT_EVALUATED` |
| `memory` | 8 | `UNSCORED` | `NOT_EVALUATED` |
| `context` | 8 | `UNSCORED` | `NOT_EVALUATED` |
| `gateway` | 8 | `UNSCORED` | `NOT_EVALUATED` |
| `channels` | 8 | `UNSCORED` | `NOT_EVALUATED` |
| `mcp` | 6 | `UNSCORED` | `NOT_EVALUATED` |
| `acp` | 5 | `UNSCORED` | `NOT_EVALUATED` |
| `environments` | 6 | `UNSCORED` | `NOT_EVALUATED` |
| `batch_eval` | 5 | `UNSCORED` | `NOT_EVALUATED` |
| `release` | 5 | `UNSCORED` | `NOT_EVALUATED` |
| `community` | 3 | `UNSCORED` | `NOT_EVALUATED` |
| **Total** | **100** | **UNSCORED** | **0 verified slots** |

The weights deliberately favor the daily product loop: tools, runtime
surfaces, session continuity, memory/context, gateway, and channels. Novel
architecture does not compensate for a broken first run or an unusable turn.

## Gate Definitions

### Parity

Parity means the Zaion behavior is implemented on a supported user path,
matches the pinned Hermes behavior for the stated acceptance criteria, and has
local verification evidence. A command name, type, crate, plan, or source-only
inspection is not parity.

### Surpass

Surpass means parity is already closed and Zaion has a measured advantage for
the same user job. The advantage must be visible in behavior such as stronger
provenance, lower operational cost, faster recovery, safer execution, or better
task success. A feature that users cannot reach does not count.

### 10/10

A category may receive 10 only when all of the following are true:

1. Its parity, surpass, and category-specific 10/10 acceptance criteria pass.
2. Every reserved task slot represented by the scored task has verified
   evidence.
3. Evidence includes reproducible commands or artifacts and observed results.
4. The task status is `verified`, its score is `10`, and its evidence grade
   is `release_verified`.
5. No release-blocking threat in `docs/THREAT_MODEL.md` contradicts the claim.

The validator rejects a 10 that lacks those conditions. There is no inferred,
estimated, rounded-up, or roadmap-derived 10.

## Scoring Scale

| Score | Meaning |
| ---: | --- |
| 0 | Missing or contradicted by current evidence |
| 1-2 | Prototype or source-only shape; no reliable user path |
| 3-4 | Partial local behavior with major workflow gaps |
| 5-6 | Useful beta behavior; parity remains incomplete |
| 7 | Hermes parity closed for the category |
| 8 | Parity plus one measured Zaion advantage |
| 9 | Broad surpass evidence across supported platforms and recovery paths |
| 10 | Release-grade category exit conditions verified in full |

Until executable cases replace the planned task-family slots, category and
overall scores remain `UNSCORED`. Missing evidence is not silently converted
to zero because that would mix "not tested" with "tested and failed."

## Evidence Grades

| Grade | Use |
| --- | --- |
| `NOT_EVALUATED` | Planned task with no result |
| `SOURCE_ONLY` | Static inspection; useful for calibration, never sufficient for parity |
| `LOCAL_VERIFIED` | Reproducible local command and observed result |
| `CROSS_PLATFORM_VERIFIED` | Required OS/client/channel matrix passed |
| `RELEASE_VERIFIED` | Packaged artifact or supported deployment passed the full exit gate |

Evidence must identify its artifact path, result, observation time, baseline
commit, and relevant acceptance criterion. Screenshots and prose may support
evidence, but cannot replace an executable result where one is possible.

## Calculation

After task slots are materialized and evaluated:

`overall = sum(category_score * category_weight) / 100`

Within each executable mission, the market-facing risk-adjusted result is:

`mission = task_success * 40% + no_human_rework * 20% + recovery * 15% + trust_verification * 15% + cost_latency * 10%`

Competitors must run the same task, model, budget, environment, and timeout.
Feature count, command count, source line count, and internal proof count are
not substitutes for this mission result. A future market-surpass claim requires
the benchmark-level confidence interval and competitor-version metadata in
addition to the evidence rules above.

The overall score is publishable only when every category has a numeric score.
Parity and surpass labels are independent gates; a high weighted average cannot
hide a failed security, identity, release, or cross-principal isolation gate.

## Update Protocol

1. Change benchmark sources only in a new manifest version.
2. Move a task from `planned` to `ready` only when its fixture and command
   exist.
3. Record `running` only during an actual evaluation.
4. Record `verified` only with complete evidence and a matching result.
5. Run `scripts/validate-product-gates.ps1` before publishing any score.
6. Keep historical comparison ledgers unchanged unless their own scope is
   explicitly recalibrated.
