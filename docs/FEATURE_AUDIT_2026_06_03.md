# Zaion Feature Audit Report

**Date:** 2026-06-03  
**Purpose:** Comprehensive inventory of all 31 crates to support strategic decision-making

---

## Executive Summary

**Total Crates:** 31  
**Total Lines of Code:** ~191,550  
**Core Vision:** Proactive agent that can initiate conversations autonomously

### Status Distribution
| Status | Count | % |
|--------|-------|---|
| Stable | 9 | 29% |
| Beta | 11 | 36% |
| Experimental | 11 | 35% |

### Relevance to Proactive Agent Vision
| Relevance | Count | % |
|-----------|-------|---|
| High | 20 | 65% |
| Medium | 10 | 32% |
| Low | 1 | 3% |

---

## Complete Crate Inventory

| Crate | Purpose | Status | Relevance | LOC | Recommendation |
|-------|---------|--------|-----------|-----|----------------|
| zaion-paths | Environment path resolution | Stable | High | 265 | Keep |
| zaion-types | Shared type definitions | Stable | High | 1,013 | Keep |
| zaion-crypto | Ed25519 keypair, DID, signatures | Stable | High | 416 | Keep |
| zaion-ledger | Event ledger with SQLite backend | Stable | High | 2,690 | Keep |
| zaion-memory | Semantic search, HNSW indexing | Stable | High | 3,453 | Keep |
| zaion-runtime | Core agent loop, MCP bridge | Stable | High | 23,968 | Keep |
| zaion-contract-macros | Compile-time contract enforcement | Stable | Medium | 332 | Keep |
| zaion-adapters | Multi-channel support (Telegram, Discord, etc.) | Stable | High | 21,390 | Keep |
| zaion-core | Daemon/process management, IPC | Stable | High | 1,682 | Keep |
| zaion-cli | Command-line interface (50+ commands) | Stable | High | 97,502 | Keep (refactor) |
| zaion-secrets | Secret management, encryption | Stable | High | 636 | Keep |
| zaion-pricing | LLM usage cost estimation | Stable | Medium | 813 | Keep |
| zaion-safety | Prompt injection, secret redaction | Stable | High | 1,034 | Keep |
| zaion-tui | Terminal UI (topology, processes) | Beta | Medium | 1,755 | Keep |
| zaion-codex | AST parsing, codebase indexing | Beta | Medium | 2,710 | Keep |
| zaion-gitledger | Git-native ledger with time-travel | Beta | Medium | 854 | Keep |
| zaion-mcp | MCP tool registry and dispatcher | Beta | High | 3,010 | Keep |
| zaion-watchdog | Ouroboros self-healing protocol | Beta | High | 2,129 | Keep |
| zaion-aci | Agent Computer Interface 2.0 | Beta | High | 2,422 | Keep |
| zaion-shadow | Background task executor | Beta | High | 2,190 | Keep |
| zaion-sync | Ledger export/import bundles | Beta | Medium | 1,366 | Keep |
| zaion-checkpoint | Write-before file snapshots | Beta | Medium | 536 | Keep |
| zaion-a2a | Agent-to-agent communication | Beta | High | 2,453 | Keep |
| zaion-telemetry | Context chain tracing | Beta | High | 585 | Keep |
| zaion-gateway | HTTP/WebSocket gateway | Beta | Medium | 356 | Keep |
| zaion-proptest | Property-based testing | Experimental | Low | 541 | Evaluate |
| zaion-ego | Personality manifest (System I) | Experimental | High | 603 | Keep → Beta |
| zaion-autonomic | Reflexive system (System II) | Experimental | High | 806 | Keep → Beta |
| zaion-proprioception | Hardware fingerprinting (System III) | Experimental | High | 677 | Keep → Beta |
| zaion-metabolic | Token budgeting (System IV) | Experimental | High | 851 | Keep → Beta |
| zaion-curiosity | Idle ideation (System V) | Experimental | High | 762 | Keep → Beta |
| zaion-singularity | Unified orchestration (I-V) | Experimental | High | 422 | Keep → Beta |
| zaion-evolve | Self-evolution engine | Experimental | High | 4,395 | Keep → Beta |
| zaion-opd | On-Policy Distillation | Experimental | High | 8,314 | Keep (split) |
| zaion-enclave | Software TEE | Experimental | Medium | 524 | Evaluate |
| zaion-federation | Honcho client, federated sessions | Experimental | Medium | 870 | Evaluate |

---

## 🤖 Autonomy & Consciousness (THE DIFFERENTIATOR)

**6 crates, 4,121 LOC** — **Critical for "Proactive Agent" Vision**

These are what make Zaion unique:

### System I: Ego (603 LOC)
- Personality manifest (ego.toml)
- Lexical baffle, soul hash
- Response filtering

### System II: Autonomic (806 LOC)
- Zero-token reflexive responses
- Action potentials, stimulus thresholds
- WASM probes

### System III: Proprioception (677 LOC)
- Hardware fingerprinting
- Transplantation shock detection
- Lockdown mechanisms

### System IV: Metabolic (851 LOC)
- Token budgeting
- Pain receptors
- Hunger-driven degradation

### System V: Curiosity (762 LOC)
- **Idle timer**
- **Autonomous ideation loop**
- **This is the core "proactive conversation" trigger**

### Singularity (422 LOC)
- Unified orchestration of Systems I-V

**Status:** All Experimental  
**Recommendation:** **Priority 1 - Move to Beta in 3 months**

---

## Key Strengths

1. ✅ **Complete autonomy stack:** Systems I-V form coherent consciousness architecture
2. ✅ **Multi-channel production:** Telegram fully functional (13K LOC)
3. ✅ **Self-healing:** Watchdog + Ouroboros enables crash recovery
4. ✅ **Signed ledger:** All operations auditable with Ed25519
5. ✅ **Self-evolution:** Both OPD (training) and Evolve (code generation)
6. ✅ **Safe code modification:** ACI 2.0 with syntax validation

---

## Gaps for "Proactive Agent" Vision

1. ❌ **Initiative model unclear:** What triggers spontaneous conversation?
2. ❌ **No scheduled autonomy:** No recurring proactive task scheduler
3. ❌ **Documentation missing:** How does curiosity work?
4. ❌ **User control unclear:** How to enable/disable/configure proactive mode?

---

## Strategic Recommendations

### Priority 1: Move Autonomy to Beta (3 months)
1. Integration tests for all 6 autonomy crates
2. Doctor checks for each system
3. **Documentation:** `docs/PROACTIVE_BEHAVIOR.md`
4. **User control:** CLI commands for enable/disable/configure

### Priority 2: Simplify Onboarding (2 weeks)
1. Clear value proposition in README
2. Animated demo GIF showing proactive conversation
3. `zaion quickstart` command

### Priority 3: Federation Testing (1 month)
1. Integrate zaion-federation with zaion-a2a
2. Multi-agent coordination tests

---

## Recommended Actions

**Keep:** 28 crates (~189K LOC)  
**Evaluate:** 3 crates (enclave, federation, proptest)  
**Cut:** None (yet)

All crates serve a purpose. Focus on **moving autonomy from Experimental to Beta**.
