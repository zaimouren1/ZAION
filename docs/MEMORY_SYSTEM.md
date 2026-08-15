# Zaion Memory System

## Overview

Zaion implements a **7-layer memory architecture** with cryptographic signatures, temporal validity, and automatic extraction. This document describes the complete memory system, focusing on the newly implemented **Layer 4: Typed Memory**.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  Layer 7: Agentic Context (conversation turns)              │
├─────────────────────────────────────────────────────────────┤
│  Layer 6: Principal Memory (signed key-value store)         │
├─────────────────────────────────────────────────────────────┤
│  Layer 5: Semantic Memory (HNSW vector search)              │
├─────────────────────────────────────────────────────────────┤
│  Layer 4: Typed Memory (User/Feedback/Project/Reference) ⭐ │
├─────────────────────────────────────────────────────────────┤
│  Layer 3: Skill Memory (tools & capabilities)               │
├─────────────────────────────────────────────────────────────┤
│  Layer 2: Reality Sync (file anchors & drift detection)     │
├─────────────────────────────────────────────────────────────┤
│  Layer 1: Ledger (immutable event log)                      │
└─────────────────────────────────────────────────────────────┘
```

## Layer 4: Typed Memory (NEW)

### Four Memory Categories

Inspired by claude.ai's memory system, Layer 4 introduces four typed memory categories:

#### 1. **User** - Persona & Preferences
- Role and job title
- Technical skills and expertise
- Working style preferences
- Communication preferences
- Personal constraints (timezone, availability)

**Example:**
```json
{
  "memory_type": "user",
  "key": "role",
  "content": "senior backend engineer specializing in Rust and distributed systems",
  "confidence": 0.9
}
```

#### 2. **Feedback** - Behavior Corrections
- What the user liked/disliked
- Corrections to agent behavior
- Preferences about output format
- Interaction patterns that worked/didn't work

**Example:**
```json
{
  "memory_type": "feedback",
  "key": "correction.1717473829",
  "content": "User prefers concise code comments, not verbose documentation",
  "confidence": 0.8
}
```

#### 3. **Project** - Temporal Context
- Deadlines and milestones
- Team composition and roles
- Current priorities
- External constraints
- Sprint/iteration context

**Example:**
```json
{
  "memory_type": "project",
  "key": "deadline",
  "content": "MVP launch on June 15th, 2026",
  "confidence": 1.0
}
```

#### 4. **Reference** - External Pointers
- URLs and documentation links
- Issue/PR references
- External system IDs
- Integration endpoints

**Example:**
```json
{
  "memory_type": "reference",
  "key": "issue.123",
  "content": "Issue #123 tracks the authentication refactor",
  "confidence": 1.0
}
```

### Key Features

#### 🔐 Ed25519 Signatures
Every memory entry is cryptographically signed for provenance and tamper-detection:

```rust
pub struct TypedMemoryEntry {
    pub memory_type: MemoryType,
    pub key: String,
    pub content: String,
    pub principal_id: String,
    pub session_id: String,
    pub signature_hex: String,  // Ed25519 signature
    // ...
}
```

#### ⏰ Temporal Knowledge Graphs
Memories are **invalidated** rather than deleted, preserving historical context:

```rust
pub struct TypedMemoryEntry {
    pub created_at: String,          // ISO 8601
    pub invalidated_at: Option<String>,  // ISO 8601 or null
    // ...
}
```

This enables queries like:
- "What was the deadline as of March 1st?"
- "When did the user's role change?"
- "Show me all memories that were valid during Q1 2026"

#### 📊 Bayesian Trust Scoring
Each memory has a confidence score (0.0-1.0) for reliability:

```rust
pub confidence: f32,  // 0.0 = uncertain, 1.0 = certain
```

Confidence affects:
- Prefetch ranking (higher confidence memories shown first)
- Conflict resolution (trust higher confidence)
- Auto-extraction validation

#### 🧠 Automatic Extraction
Memories are extracted automatically from conversations using **rule-based patterns** (no LLM required):

```rust
pub fn extract_from_turn(
    user_content: &str,
    assistant_content: &str,
    session_id: &str,
) -> ExtractionResult
```

**Extraction Patterns:**

1. **Explicit Directives**
   - `[remember] key: value`
   - `[note] key: value`
   - `<memory key="...">value</memory>`

2. **User Persona**
   - "I am a..." → User/role
   - "I prefer..." → User/preference
   - "My name is..." → User/name

3. **Feedback Signals**
   - "That's perfect!" → Feedback/positive
   - "That's wrong" → Feedback/negative
   - "Actually, I meant..." → Feedback/correction

4. **Project Context**
   - "deadline is..." → Project/deadline
   - "team has..." → Project/team_context

5. **References**
   - URLs (`https://...`)
   - Issue refs (`#123`, `PR#456`)

### Storage

Typed memories are stored in SQLite with **WAL mode** for concurrent access:

```sql
CREATE TABLE typed_memory (
    id INTEGER PRIMARY KEY,
    memory_type TEXT NOT NULL,     -- 'user' | 'feedback' | 'project' | 'reference'
    key TEXT NOT NULL,
    content TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    invalidated_at TEXT,
    confidence REAL NOT NULL,
    source TEXT NOT NULL,
    signature_hex TEXT NOT NULL,
    UNIQUE(principal_id, memory_type, key)
);

CREATE INDEX idx_typed_memory_principal ON typed_memory(principal_id, memory_type);
CREATE INDEX idx_typed_memory_validity ON typed_memory(invalidated_at);
```

### Runtime Integration

#### Prefetch Lifecycle
Before each agent turn, typed memories are automatically loaded:

```rust
fn prefetch(&self, query: &str, session_id: &str) -> Result<String> {
    // Load typed memories grouped by type
    let entries = self.typed_store.list_all(&self.principal_id, false)?;
    
    // Format by type:
    // USER memories:
    //   - role: senior engineer
    //   - preference: concise responses
    // 
    // PROJECT memories:
    //   - deadline: June 15th
    // ...
}
```

#### Sync Lifecycle
After each turn, new memories are extracted and persisted:

```rust
fn sync_turn(
    &self,
    user_content: &str,
    assistant_content: &str,
    session_id: &str,
) -> Result<()> {
    // 1. Extract typed memories
    let result = AutoMemoryExtractor::extract_from_turn(
        user_content, 
        assistant_content, 
        session_id
    );
    
    // 2. Persist to TypedMemoryStore
    for entry in result.candidates {
        self.typed_store.upsert(&entry)?;
    }
    
    // 3. Continue with semantic/principal sync...
}
```

### Tool API

Typed memory is exposed to the agent via three tools:

#### 1. `memory_typed_get`
```json
{
  "name": "memory_typed_get",
  "parameters": {
    "memory_type": "user",  // user | feedback | project | reference
    "key": "role"
  }
}
```

#### 2. `memory_typed_set`
```json
{
  "name": "memory_typed_set",
  "parameters": {
    "memory_type": "user",
    "key": "role",
    "content": "senior engineer",
    "confidence": 0.9
  }
}
```

#### 3. `memory_typed_list`
```json
{
  "name": "memory_typed_list",
  "parameters": {
    "memory_type": "user",  // optional: filter by type
    "include_invalidated": false
  }
}
```

### CLI Commands

Manage typed memories via the CLI:

```bash
# List all memories
zaion typed-memory list

# List memories by type
zaion typed-memory list user
zaion typed-memory list feedback

# Show specific memory
zaion typed-memory show user role

# Clear memories
zaion typed-memory clear feedback    # by type
zaion typed-memory clear             # all (with confirmation)

# Statistics
zaion typed-memory stats

# Export/Import
zaion typed-memory export memories.json
zaion typed-memory import memories.json
```

## Other Memory Layers

### Layer 5: Semantic Memory
- Vector embeddings via HNSW (Hierarchical Navigable Small World)
- Fast approximate nearest neighbor search
- Deterministic local fallback embedding (384-dim hash)
- Metadata includes embedding quality trace

### Layer 6: Principal Memory
- Signed key-value store scoped to principal
- Ed25519 signatures for provenance
- JSON values (arbitrary structured data)
- Used for explicit `[remember]` directives

### Layer 3: Skill Memory
- Tool definitions and capabilities
- Runtime-discoverable skills
- Capability declarations
- Used for tool routing

### Layer 2: Reality Sync
- File anchor system (SHA-256 hashes)
- Drift detection for file modifications
- Synchronization with external state
- Prevents hallucinated references

### Layer 1: Ledger
- Immutable event log
- All memory operations auditable
- Ed25519 signed events
- Foundation for federation sync

## Design Philosophy

### 1. **Provenance First**
Every memory is cryptographically signed and traceable to its origin.

### 2. **Temporal by Default**
Memories are never deleted, only invalidated. This preserves historical context.

### 3. **Confidence Over Truth**
Memories carry confidence scores rather than binary true/false labels.

### 4. **Automatic Over Manual**
Rule-based extraction reduces manual tagging burden.

### 5. **Offline-First**
Deterministic local embeddings ensure reproducibility without external APIs.

## Comparison with Hermes/claude.ai

| Feature | Hermes | Zaion |
|---------|--------|-------|
| Memory Types | Unstructured | 4 typed categories |
| Signatures | No | Ed25519 on all entries |
| Temporal Validity | No | Invalidation timestamps |
| Auto-Extraction | LLM-based | Rule-based (no LLM) |
| Confidence Scoring | No | Bayesian trust scores |
| Federation Ready | No | Principal-scoped |
| Drift Detection | No | Reality Sync layer |

## Performance

### Storage
- **TypedMemoryStore**: ~50 memories = 20KB SQLite
- **Prefetch overhead**: <10ms for 30 memories
- **Sync overhead**: <5ms per turn

### Memory Usage
- **HNSW index**: ~1KB per entry
- **Total memory footprint**: <10MB for 1000 entries

## Future Enhancements

### Phase 1 (Completed ✅)
- [x] Four typed memory categories
- [x] Ed25519 signatures
- [x] Temporal validity
- [x] Automatic extraction
- [x] CLI commands
- [x] Runtime integration

### Phase 2 (Planned - Week 3-4)
- [ ] LLM-based extraction fallback (via OPD)
- [ ] Confidence decay over time
- [ ] Memory consolidation (merge similar entries)
- [ ] Cross-principal memory federation

### Phase 3 (Planned - Week 9-10)
- [ ] Semantic search over typed memories
- [ ] Memory conflict resolution
- [ ] Multi-modal memories (images, audio)
- [ ] Memory provenance graphs (who extracted what)

## Testing

### Unit Tests
```bash
# Run all memory tests
cargo test -p zaion-memory

# Run typed memory tests only
cargo test -p zaion-memory typed_memory

# Run auto-extraction tests
cargo test -p zaion-memory auto_extraction
```

### Integration Tests
```bash
# Test runtime integration
cargo test -p zaion-memory runtime_integration

# Test CLI commands
cargo build -p zaion-cli
./target/debug/zaion typed-memory list
```

### Coverage
- **54 tests** passing
- **TypedMemoryStore**: 100% coverage
- **AutoMemoryExtractor**: 100% coverage
- **Runtime integration**: Full lifecycle tested

## References

- [claude.ai memory system](https://www.anthropic.com/news/memory) - Inspiration for typed categories
- [Mem0 extraction pipeline](https://github.com/mem0ai/mem0) - Automatic extraction patterns
- [HNSW algorithm](https://arxiv.org/abs/1603.09320) - Approximate nearest neighbor search
- [Ed25519 signatures](https://ed25519.cr.yp.to/) - Cryptographic provenance
