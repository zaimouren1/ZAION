# Zaion Self-Healing System (Ouroboros Protocol)

## Overview

Zaion implements the **Ouroboros Protocol** — a cryptographically signed, fully auditable self-healing system that automatically detects crashes, generates fixes via LLM, and restarts the main process with zero human intervention.

```
┌─────────────────────────────────────────────────────────────┐
│  Ouroboros Self-Healing Cycle                               │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  1. ProcessMonitor  →  Heartbeat detection (PID file)      │
│  2. CrashDetector   →  Capture stack trace + damaged files │
│  3. CrashHealer     →  LLM generates repair plan           │
│  4. Resurrector     →  Apply fix + Restart process         │
│  5. RepairHistory   →  Cryptographic audit trail           │
│  6. Ledger          →  Immutable event log                 │
│                                                             │
│  → "We are back online."                                    │
└─────────────────────────────────────────────────────────────┘
```

## Architecture

### Core Components

#### 1. **ProcessMonitor** (src/monitor.rs)
- Monitors main process via PID file
- Heartbeat detection: polls every 2 seconds
- Returns `Alive`, `Dead`, or `NoPidFile` status

#### 2. **CrashDetector** (src/crash.rs)
- Captures crash stack traces from stderr logs
- Identifies damaged files (parse errors, missing files)
- Generates `CrashReport` with:
  - Stack trace
  - Damaged file paths
  - Crash timestamp
  - Exit code
  - One-line summary

#### 3. **CrashHealer** (src/healer.rs)
- Sends crash report to OpenAI-compatible LLM
- Parses LLM response into `HealPlan`
- Three fix types:
  - `FileContent`: LLM returns corrected file (auto-apply)
  - `Description`: LLM returns steps (manual intervention)
  - `Unknown`: LLM cannot fix (manual intervention)

#### 4. **Resurrector** (src/resurrect.rs)
- Applies fix from `HealPlan`
- Creates backup before overwriting (`.bak` extension)
- Restarts main process with original arguments
- Records repair to `RepairHistory`
- Returns `ResurrectResult` with:
  - `fix_action`: What was done
  - `new_pid`: Process ID after restart (if successful)
  - `message`: Human-readable status
  - `repair_entry_id`: Audit trail ID

#### 5. **RepairHistory** (src/history.rs)
- SQLite-backed repair audit trail
- Each entry contains:
  - Crash summary (first 500 chars)
  - Fix type and content
  - File path (if applicable)
  - Result (Success/ManualRequired/Failure)
  - New PID (if restarted)
  - Principal ID (who performed repair)
  - **Ed25519 signature** for provenance
- Methods:
  - `add()`: Record new repair
  - `get(id)`: Retrieve by ID
  - `list(limit)`: Most recent repairs (DESC order)
  - `count()`: Total repairs
  - `count_by_result()`: Count by Success/Manual/Failure
  - `latest()`: Most recent repair
  - `clear()`: Delete all (dangerous)

#### 6. **LedgerWriter** (src/ledger_writer.rs)
- Writes immutable events to Ledger
- Event types:
  - `system.crash_detected`
  - `system.resurrection`
- Ed25519 signed for federation sync

## Usage

### CLI Commands

#### Start Watchdog

```bash
# Foreground (blocks terminal)
zaion watchdog start

# Background (detached process)
zaion watchdog start --background
```

#### Check Status

```bash
zaion watchdog status

# Output examples:
# ✓ zaion main process alive  (pid=12345)
# ✗ zaion main process dead   (last pid=12345)
# ? zaion main process not started (no PID file)
```

#### View Repair History

```bash
# Show last 20 repairs
zaion watchdog history

# Show last 50 repairs
zaion watchdog history 50

# Output format:
# ID   Timestamp            Result          Fix Type     Summary
# ────────────────────────────────────────────────────────────────
# 3    2026-06-03 14:32:15  ✓ Success      file_content  failed to parse config.toml
# 2    2026-06-03 12:18:03  ⚠ Manual       description   Missing dependency: libssl
# 1    2026-06-03 10:05:42  ✓ Success      file_content  invalid TOML syntax

# Statistics:
# Total repairs: 3
#   ✓ Success: 2
#   ⚠ Manual:  1
#   ✗ Failure: 0
```

#### View Ledger Events

```bash
# Show last 20 self-heal events
zaion watchdog logs

# Show last 50 events
zaion watchdog logs 50
```

#### Manual Drill (Testing)

```bash
# Test repair without LLM
zaion watchdog drill damaged.toml --candidate fixed.toml --pid <principal-id>

# Creates backup, applies fix, signs receipt
```

### Programmatic Usage

```rust
use zaion_watchdog::{
    ProcessMonitor, CrashDetector, CrashHealer, Resurrector,
    RepairHistory, WatchdogConfig,
};
use zaion_crypto::keypair::ZaionKeypair;

// 1. Monitor process
let config = WatchdogConfig::default_local();
let monitor = ProcessMonitor::new(config.clone());

match monitor.watch_until_death() {
    Ok(dead_pid) => {
        eprintln!("Process died: {}", dead_pid);

        // 2. Detect crash
        let detector = CrashDetector::new(
            config.crash_log_dir.clone(),
            config.config_file.clone(),
        );
        let crash_report = detector.detect()?;

        // 3. Generate fix via LLM
        let healer = CrashHealer::new(config.clone());
        let heal_plan = healer.heal(&crash_report).await?;

        // 4. Apply fix + restart + record history
        let history_dir = config.crash_log_dir.join("repair_history");
        let history = RepairHistory::new(&history_dir);
        let keypair = ZaionKeypair::generate(); // or load from storage
        let resurrector = Resurrector::new(config, history, keypair);

        let result = resurrector.resurrect(&crash_report, &heal_plan)?;
        eprintln!("✓ {}", result.message);
        eprintln!("  Repair ID: {}", result.repair_entry_id);
        if let Some(pid) = result.new_pid {
            eprintln!("  New PID: {}", pid);
        }
    }
    Err(e) => eprintln!("Monitor error: {}", e),
}
```

## Configuration

### WatchdogConfig

```rust
pub struct WatchdogConfig {
    pub main_binary: PathBuf,       // Path to main zaion binary
    pub main_args: Vec<String>,     // Args to restart with
    pub pid_file: PathBuf,          // PID file location
    pub crash_log_dir: PathBuf,     // Stderr logs location
    pub config_file: PathBuf,       // Main config file
    pub llm_endpoint: String,       // OpenAI-compatible endpoint
    pub llm_api_key: String,        // API key
    pub llm_model: String,          // Model name (default: gpt-4)
    pub ledger_db_path: PathBuf,    // Ledger SQLite path
    pub max_heal_attempts: usize,   // Max retries (default: 3)
}
```

### Default Locations

- **PID file**: `~/.zaion/zaion.pid`
- **Crash logs**: `~/.zaion/logs/stderr.log`
- **Repair history**: `~/.zaion/logs/repair_history/repair_history.db`
- **Ledger**: `~/.zaion/ledger/events.db`

## Repair History Schema

### SQLite Table

```sql
CREATE TABLE repair_history (
    id INTEGER PRIMARY KEY,
    timestamp TEXT NOT NULL,           -- ISO 8601
    crash_summary TEXT NOT NULL,       -- First 500 chars
    fix_type TEXT NOT NULL,            -- 'file_content' | 'description' | 'unknown'
    fix_content TEXT NOT NULL,         -- Fix applied
    file_path TEXT,                    -- Affected file
    result TEXT NOT NULL,              -- 'success' | 'manual_required' | 'failure'
    new_pid INTEGER,                   -- PID after restart
    principal_id TEXT NOT NULL,        -- Who performed repair
    signature_hex TEXT NOT NULL        -- Ed25519 signature
);

CREATE INDEX idx_repair_timestamp ON repair_history(timestamp);
CREATE INDEX idx_repair_result ON repair_history(result);
```

### Signature Verification

Each repair entry is signed with Ed25519:

```rust
let entry = history.get(repair_id)?;
entry.verify(&keypair)?; // Throws error if signature invalid
```

Canonical message format:
```
timestamp|crash_summary|fix_type|fix_content|file_path|result|principal_id
```

## LLM Prompt Format

The healer sends this prompt to the LLM:

```
Zaion process crashed with the following error:

Summary: failed to parse config.toml

Affected files: /home/user/.zaion/config.toml

Stack trace (truncated):
thread 'main' panicked at 'failed to parse config.toml'
Caused by: invalid TOML syntax at line 5
File: /home/user/.zaion/config.toml

Please provide the corrected file content for the TOML/JSON config file 
if this is a parse error, or a brief fix description otherwise. 

Respond with JSON: 
{
  "fix_type": "file_content" | "description", 
  "file_path": "<path>", 
  "content": "<corrected content or fix steps>"
}
```

## Testing

### Unit Tests

```bash
# Run all watchdog tests
cargo test -p zaion-watchdog

# Run specific module tests
cargo test -p zaion-watchdog --lib history
cargo test -p zaion-watchdog --lib resurrect
```

### Integration Tests

```bash
# Run Ouroboros end-to-end tests
cargo test -p zaion-watchdog --test ouroboros

# 5 integration tests:
# - test_ouroboros_full_cycle_file_content
# - test_ouroboros_manual_intervention_required
# - test_ouroboros_unknown_fix
# - test_ouroboros_multiple_repairs_tracked
# - test_ouroboros_repair_history_statistics
```

### Manual Testing

```bash
# 1. Start main process
zaion process start

# 2. Start watchdog (different terminal)
zaion watchdog start

# 3. Simulate crash (kill main process)
kill -9 <pid>

# 4. Watch watchdog resurrect the process
# Output:
# [zaion-watchdog] ⚡ Main process (pid=12345) died. Ouroboros activated.
# [zaion-watchdog] Crash summary: failed to parse config.toml
# [zaion-watchdog] ✓ Config corruption detected and self-healed. We are back online.
# [zaion-watchdog] Repair entry logged: ID 1

# 5. Check repair history
zaion watchdog history
```

## Performance

### Storage

- **RepairHistory SQLite**: ~10KB per 50 repairs
- **Ledger events**: ~2KB per crash/resurrection pair
- **Index overhead**: <1KB per 100 entries

### Runtime

- **Heartbeat check**: <1ms
- **Crash detection**: <10ms (reads last 2000 lines of stderr)
- **LLM call**: 2-10 seconds (network dependent)
- **File overwrite**: <5ms
- **Process restart**: 100-500ms (platform dependent)
- **History write**: <5ms (SQLite WAL mode)

**Total resurrection time**: ~3-12 seconds (dominated by LLM)

## Security

### Signatures

- All repair entries signed with Ed25519
- Signature includes:
  - Timestamp
  - Crash summary
  - Fix content
  - Result
  - Principal ID
- Prevents tampering with audit trail

### File Backups

- Automatic `.bak` backup before overwrite
- Original file preserved even if LLM fix is wrong
- Manual rollback: `mv file.bak file.ext`

### Ledger Immutability

- All resurrection events written to append-only ledger
- Cannot be deleted or modified
- Federation-ready for multi-agent coordination

## Failure Modes

### LLM Unavailable

- Falls back to cold restart (no fix applied)
- Logs failure to repair history
- Max 3 attempts before giving up

### Invalid LLM Response

- Parsed as `HealFixType::Unknown`
- Marks repair as `ManualRequired`
- No file overwrite

### Process Won't Restart

- Logs failure to repair history
- Returns error to caller
- Manual intervention required

### File Changed During Repair

- Reality sync hash check fails
- Refuses to overwrite
- Logs as `Failure`

## Roadmap

### Phase 1 (Completed ✅)

- [x] RepairHistory module with Ed25519 signatures
- [x] Resurrector integration
- [x] End-to-end integration tests
- [x] CLI `zaion watchdog history` command

### Phase 2 (Week 3 Day 3-5)

- [ ] Doctor health checks
- [ ] Repair statistics dashboard
- [ ] Confidence decay over time
- [ ] Memory consolidation (merge similar repairs)

### Phase 3 (Week 5-6)

- [ ] LLM fallback: local model if API unavailable
- [ ] Multi-modal repairs (image corruption, audio files)
- [ ] Provenance graphs (who extracted what)
- [ ] Cross-principal federation

## References

- [Ed25519 signatures](https://ed25519.cr.yp.to/)
- [SQLite WAL mode](https://www.sqlite.org/wal.html)
- [Ouroboros (mythology)](https://en.wikipedia.org/wiki/Ouroboros) - Snake eating its own tail, symbolizing self-sufficiency and eternal return

## FAQ

**Q: What if the LLM gives a bad fix?**  
A: Backups are created automatically. Rollback: `mv file.bak file.ext`. The repair is logged as `Success` even if wrong — manual verification recommended for critical systems.

**Q: Can I use a local LLM?**  
A: Yes. Set `llm_endpoint` to any OpenAI-compatible API (e.g., Ollama, LM Studio).

**Q: How do I verify signature integrity?**  
A: Use `RepairEntry::verify(&keypair)` in Rust, or export history and verify offline.

**Q: Does this work on Windows?**  
A: Yes. Process spawning uses platform-specific flags (`DETACHED_PROCESS` on Windows, `setsid` on Unix).

**Q: What if the watchdog crashes?**  
A: The watchdog is designed to be minimal (no external dependencies, no network calls except LLM). If it crashes, restart manually with `zaion watchdog start`.

**Q: Can I disable auto-restart?**  
A: Set `max_heal_attempts=0` in config. The watchdog will detect crashes but not attempt resurrection.
