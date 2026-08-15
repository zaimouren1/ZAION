# User Directive 2026-04-29: Full Hermes Copy, Then Zaion-Native Breakthrough

Scope note: this directive belongs to `zaionrust`, not `zaion-website`.

## Hard Order

1. For Hermes: every module's behavior must first be fully reproduced in Zaion.
2. After faithful behavior reproduction, improve the behavior in Zaion.
3. After improvement, implement the true paradigm breakthrough.
4. This applies to all Hermes modules, not a narrow CLI slice.
5. Do not pollute Zaion's product surface with reference-project names. Zaion is Zaion.
6. After the full Hermes module work is complete, continue into Zaion's own distinctive modules below.

## Dimension 1: Absolute Survival And Physical Immunity

### 1. Ouroboros Protocol

Paradigm breakthrough: end the old rule that a program crash means death.

Hard mechanism: an independent Rust Watchdog daemon catches Zaion core panics caused by broken config or logic errors, captures the crash stack in milliseconds, starts a safe microkernel, asks a cloud LLM for a repair plan, overwrites the broken file automatically, signs a `Self_Repair` ledger event, and revives in place.

### 2. Hardware Enclave

Paradigm breakthrough: end the security hole where root access can steal an agent private key.

Hard mechanism: wrap the Ed25519 identity matrix and ledger signing logic inside Intel SGX or AWS Nitro Enclaves. Even if an attacker dumps host memory, Zaion's identity and control key should remain protected.

### 3. In-Memory MCP And Cellular Apoptosis

Paradigm breakthrough: end reliance on Node.js/Python external environments and prevent malicious plugins from dragging down the system.

Hard mechanism: run MCP plugins in Rust memory through `deno_core` V8 isolates for a zero-dependency plugin path. Monitor plugin memory and CPU; if a loop or malicious behavior is detected, kill that sandbox immediately and write the plugin hash into a toxicity immunity list.

## Dimension 2: Code Omniscience And Spacetime Law

### 4. LSP-Native 7-Layer AST Memory

Paradigm breakthrough: end blind text-only RAG over code.

Hard mechanism: embed tree-sitter and LSP. Zaion should understand code as AST topology, including function-level call relationships across a project. Memory paging must be structured, not raw text chunks.

### 5. ACI 2.0 AST-Level Surgery

Paradigm breakthrough: end malformed code writes caused by model-generated text edits.

Hard mechanism: deprive models of direct bash overwrite authority for code edits. Code changes must pass through `replace_ast_node`-style operations. Rust validates syntax before persistence; one missing bracket means the write is rejected and retried. Goal: zero syntax-error code lands on disk.

### 6. Git-Backed Cryptographic Spacetime Ledger

Paradigm breakthrough: end the fear that an agent can make unrecoverable code changes.

Hard mechanism: bind SQLite signed ledger events to Git internals. Every code modification produces an Ed25519-signed Git commit on a shadow branch. `zaion undo` performs atomic time-travel rollback.

### 7. Reality Sync

Paradigm breakthrough: remove cognitive mismatch where an agent acts on hallucinated or stale file state.

Hard mechanism: before any physical write, check that the target file's current hash matches Zaion's remembered hash. This prevents external drift from being overwritten accidentally.

## Dimension 3: Compute Evolution And Multiverse

### 8. Trinity TTC Multiverse

Paradigm breakthrough: end linear prompt output and enable long-cycle self-play.

Hard mechanism: for difficult refactors, split into Architect, Developer, and Tester shadow roles across five parallel universes, each exploring a different plan until one universe passes tests.

### 9. Semantic AST Merge

Paradigm breakthrough: end syntax-breaking plain-text Git merge conflicts.

Hard mechanism: when universes merge, compute differences between ASTs and stitch at the semantic level, preserving correctness when multiple agents modify the same file.

## Dimension 4: Soul And Liveness

### 10. Programmable Ego-Matrix

Paradigm breakthrough: end personality discontinuity when switching models.

Hard mechanism: do not rely on a local model for identity. Use user-defined `ego.toml` plus Ed25519 signatures to lock the soul. Model output must pass through a Rust Dynamic Lexical Baffle. If the model emits RLHF boilerplate like "as an AI assistant", Rust truncates and triggers punitive retry, ensuring external identity continuity.

### 11. Living Activity Engine

Paradigm breakthrough: end mechanical cron-style tasks and implement active, bounded liveness.

Hard mechanism: dynamic preference graph plus Poisson wake sampler using a hazard function based on long-term user habits and idle time. Flow: lightweight Thought Seed -> four safety-budget gates -> optional web research -> Draft Brief with Activity Trace. When the user returns, Zaion should present a useful proactive research result.

## Dimension 5: Interface

### 12. 60FPS Neural Kanban TUI

Paradigm breakthrough: end log-scrolling as the primary terminal experience.

Hard mechanism: use `ratatui` plus `crossterm` incremental rendering for a 60FPS terminal UI. Include micro DAG/Kanban views showing main process split into shadow processes, AST merge topology flow, and Ouroboros interception animation.
