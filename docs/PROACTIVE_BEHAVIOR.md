# Proactive Behavior System

> **Systems I-V: Autonomous Agent Architecture**  
> Version: 1.0.0-beta  
> Status: Experimental → Beta (2026-06-05)

---

## Overview

Zaion's **Proactive Behavior System** is what differentiates it from traditional reactive AI agents. Instead of merely responding to user input, Zaion actively initiates conversations, explores ideas, and suggests improvements during idle periods.

This system is powered by **Systems I-V**, six interconnected modules that form a consciousness-like architecture:

| System | Name | Purpose |
|--------|------|---------|
| **I** | Ego-Matrix | Programmable personality via `ego.toml` |
| **II** | Autonomic Reflexes | Zero-token reflex responses |
| **III** | Hardware Proprioception | Environment fingerprinting & shock detection |
| **IV** | Metabolic Engine | Token budget tracking & degradation |
| **V** | Entropic Curiosity | Idle detection & spontaneous ideation |
| **Integration** | Singularity Runtime | Orchestration layer for Systems I-V |

---

## System I: Ego-Matrix

### What It Does

The Ego-Matrix defines Zaion's **personality, behavior, and identity** through a declarative configuration file (`ego.toml`). This is not just a system prompt — it's a signed, cryptographically verifiable identity.

### Configuration (`ego.toml`)

```toml
[soul]
name = "Zaion"
core_tone = "Direct, warm, and technically precise"

[baffle.immune_system]
banned_exact = []  # Exact tokens to filter from output
banned_regex = []  # Regex patterns to filter

[baffle.behavior]
proactive_rate = 0.3        # 30% chance to initiate conversation when idle
max_words_per_reply = 500   # Soft limit on response length
```

### Key Features

1. **Soul Hash**: Every ego.toml is hashed and signed with Ed25519
2. **Lexical Baffle**: Filter output tokens based on banned patterns
3. **XML Compilation**: Converts ego.toml → XML system prompt
4. **Behavioral Tuning**: Control proactivity, verbosity, and tone

### CLI Commands

```bash
# Show current configuration
zaion ego show

# Create default ego.toml
zaion ego init

# Compile to XML system prompt
zaion ego compile

# Verify signature
zaion ego verify

# Run health check
zaion ego doctor
```

### Health Check

```bash
$ zaion ego doctor
=== System I: Ego-Matrix Health Check ===

[1/6] Checking ego.toml existence... ✓ PASS
[2/6] Checking ego.toml validity... ✓ PASS
[3/6] Checking soul.name... ✓ PASS ("Zaion")
[4/6] Checking baffle.behavior... ✓ PASS
      → proactive_rate: 0.3
      → max_words_per_reply: 500
[5/6] Checking XML compilation... ✓ PASS
      → Generated 2847 bytes of XML
[6/6] Checking Soul_Hash signature... ✓ PASS
      → Signature verified: a7f3c2d1e8b4f5a9

=== Summary ===
✓ All checks passed. System I is healthy.
```

---

## System II: Autonomic Reflexes

### What It Does

The Autonomic system provides **zero-token reflex responses** — immediate reactions to stimuli that don't consume LLM tokens. Think of it as the agent's "nervous system."

### Architecture

```
Stimulus → WASM Probe → ActionPotential → Threshold → Reflex Action
```

**Components:**
- **ReflexRegistry**: Stores reflex definitions (trigger → action mappings)
- **ActionPotential**: Neuron-style accumulator that fires when threshold exceeded
- **StimulusAccumulator**: Manages multiple action potentials
- **ProbeEngine**: Executes WASM probes to detect stimuli
- **AutonomicRuntime**: Background polling loop (default 1000ms interval)

### Example Reflex

```rust
AutonomicReflex {
    id: "memory_pressure",
    name: "Memory Pressure Handler",
    trigger: ReflexTrigger {
        trigger_type: "memory_usage",
        pattern: None,
        threshold: Some(0.8),  // Fire at 80% memory usage
    },
    action: ReflexAction {
        action_type: "compact_memory",
        parameters: json!({"target_mb": 100}),
    },
    enabled: true,
}
```

### CLI Commands

```bash
# Show autonomic status
zaion autonomic status <pid>

# Start polling demo (500ms)
zaion autonomic start <pid>

# List registered reflexes
zaion autonomic list <pid>

# Run health check
zaion autonomic doctor
```

### Health Check

```bash
$ zaion autonomic doctor
=== System II: Autonomic Reflexes Health Check ===

[1/5] Checking AutonomicRuntime initialization... ✓ PASS
[2/5] Checking ReflexRegistry... ✓ PASS
      → Registry can store reflexes
[3/5] Checking ActionPotential... ✓ PASS
      → ActionPotential threshold firing works
[4/5] Checking StimulusAccumulator... ✓ PASS
      → StimulusAccumulator can register potentials
[5/5] Checking ProbeEngine... ✓ PASS
      → ProbeEngine initializes correctly

=== Summary ===
✓ All checks passed. System II is healthy.
```

---

## System III: Hardware Proprioception

### What It Does

Hardware Proprioception detects **environment changes** (transplantation shock) and enforces **lockdown** if the agent is moved to a different machine without authorization.

### Detection Mechanism

The system collects a **fingerprint** of the host environment:
- Hostname
- OS type and version
- CPU count
- Total memory
- Environment variables hash

When the agent boots, it compares the current fingerprint to the stored baseline. If similarity drops below threshold:
- **Mild shock** (0.7-0.9): Log warning
- **Moderate shock** (0.4-0.7): Reduce functionality
- **Severe shock** (<0.4): Engage lockdown

### Lockdown State

When locked down:
- Agent refuses to execute commands
- All systems frozen except System III
- Requires unlock token to disengage

### CLI Commands

```bash
# View environment fingerprint
zaion proprioception fingerprint

# Check for shock
zaion proprioception check

# View lockdown status
zaion proprioception status

# Unlock (requires token)
zaion proprioception unlock <token>

# Run health check
zaion proprioception doctor
```

---

## System IV: Metabolic Engine

### What It Does

The Metabolic Engine tracks **token budget** and triggers **hunger-driven degradation** when the agent runs low on resources.

### Budget Tracking

```
Total Budget: 100,000 tokens
Used: 85,000 tokens
Remaining: 15,000 tokens
Utilization: 85%
```

**Thresholds:**
- **Warning** (80%): Reduce concurrency to 2 parallel tasks
- **Critical** (95%): Emergency throttle, block new requests

### Hunger Degradation

As token consumption increases, the agent experiences "hunger":

| Level | Hunger Ratio | Performance Multiplier |
|-------|--------------|------------------------|
| None | 0.0-0.2 | 1.0× |
| Mild | 0.2-0.4 | 0.9× |
| Moderate | 0.4-0.6 | 0.7× |
| Severe | 0.6-0.8 | 0.5× |
| Critical | 0.8-1.0 | 0.3× |

**Effects:**
- Response quality degrades
- Proactive behavior reduces
- Context window shrinks

### Pain Receptors

The system monitors pain signals:
- **TokenStarvation**: Budget approaching zero
- **MemoryPressure**: RAM usage high
- **ContextOverflow**: Conversation too long
- **RepeatedFailure**: Same action failing multiple times
- **TimeoutExceeded**: Operations taking too long

### CLI Commands

```bash
# Show metabolic state
zaion metabolic status <pid>

# View token budget
zaion metabolic budget <pid>

# Check hunger level
zaion metabolic hunger <pid>

# Feed tokens (simulate completion)
zaion metabolic feed <pid> <amount>

# View pain signals
zaion metabolic pain <pid>

# Run health check
zaion metabolic doctor
```

---

## System V: Entropic Curiosity

### What It Does

The Curiosity system detects **idle periods** and generates **spontaneous ideation prompts** to encourage autonomous exploration.

### Idle Detection

```
State: Active → Idle (5 min) → DeepIdle (15 min)
```

**IdleTimer tracks:**
- Time since last user interaction
- Current idle state
- Idle percentage (relative to threshold)

### Ideation Categories

When idle threshold reached, System V generates prompts in one of six categories:

1. **Exploration**: "What if we tried...?"
2. **Optimization**: "Could we make this faster?"
3. **Refactoring**: "This code structure could improve..."
4. **Documentation**: "Missing docs for..."
5. **Testing**: "Edge case not covered..."
6. **Security**: "Potential vulnerability in..."

### Prompt Generation

**Static Fallback:**
```
Category: Security
Prompt: "Review the codebase for potential security vulnerabilities.
         Focus on input validation, authentication flows, and data sanitization."
```

**LLM-Driven** (if OpenAI API key configured):
```
Category: Refactoring
Prompt: "I notice the auth.rs module has grown to 1200 lines.
         Consider extracting JWT logic into a separate jwt_handler.rs module
         and moving validation rules to a declarative schema."
         
Context: Indexed 42 AST chunks across 18 files
         Recent changes: Modified auth.rs and db.rs
```

### CLI Commands

```bash
# Show curiosity state
zaion curiosity status <pid>

# Force ideation trigger
zaion curiosity trigger <pid>

# View ideation history
zaion curiosity history <pid>

# Run health check
zaion curiosity doctor
```

### Health Check

```bash
$ zaion curiosity doctor
=== System V: Entropic Curiosity Health Check ===

[1/5] Checking IdleTimer... ✓ PASS
      → IdleTimer initializes in Active state
[2/5] Checking IdleTimer transitions... ✓ PASS
      → IdleTimer transitions to Idle correctly
[3/5] Checking IdeationLoop... ✓ PASS
      → IdeationLoop initializes and detects idle
[4/5] Checking IdeationCategory... ✓ PASS
      → All 6 ideation categories available
      → Categories: Exploration, Optimization, Refactoring,
                    Documentation, Testing, Security
[5/5] Checking prompt generation... ✓ PASS
      → Generated prompt: 287 chars
      → Category: Exploration

=== Summary ===
✓ All checks passed. System V is healthy.
```

---

## Singularity Runtime: Integration Layer

### What It Does

The **SingularityRuntime** orchestrates all five systems into a unified runtime environment.

### Initialization

```rust
use zaion_singularity::SingularityRuntime;
use zaion_ledger::EventLedger;
use zaion_crypto::keypair::ZaionKeypair;
use zaion_types::NamespaceKey;

let ledger = Arc::new(EventLedger::new(&ledger_path));
let keypair = Arc::new(ZaionKeypair::generate());
let namespace_key = NamespaceKey("my-agent".to_string());

let runtime = SingularityRuntime::new(
    &zaion_dir,
    ledger,
    keypair,
    namespace_key,
)?;
```

### Unified API

```rust
// System I: Ego
let system_prompt = runtime.system_prompt();
let is_allowed = runtime.is_token_allowed("test");
let filtered = runtime.filter_response("response text");
let soul_hash = runtime.soul_hash();

// System II: Autonomic
let actions = runtime.check_reflexes("memory_pressure", 0.9).await?;

// System III: Proprioception
let shock = runtime.check_shock()?;
let pain_signals = runtime.check_pain();

// System IV: Metabolic
runtime.consume_tokens(5000)?;
runtime.feed_tokens(3000);
let budget = runtime.remaining_budget();
let hunger = runtime.hunger_degradation();
let policy = runtime.evaluate_metabolic_policy();

// System V: Curiosity
runtime.mark_activity();
let idle_state = runtime.idle_state();
let prompt = runtime.should_ideate();
```

### Cross-System Coordination

**Example: Token exhaustion triggers curiosity cooldown**

```rust
// System IV detects critical hunger
if runtime.hunger_degradation() == DegradationLevel::Critical {
    // System V reduces ideation frequency
    runtime.set_ideation_cooldown(Duration::from_secs(3600));
}
```

**Example: Shock detection pauses all systems**

```rust
// System III detects severe shock
if runtime.check_shock()?.severity == ShockSeverity::Severe {
    // All systems enter lockdown
    runtime.engage_lockdown("Unauthorized environment detected".to_string());
}
```

---

## Configuration

### Enable Proactive Mode

In `ego.toml`:

```toml
[baffle.behavior]
proactive_rate = 0.3  # 30% chance to initiate when idle
```

### Ideation Settings

In `ZAION_HOME/config.toml`:

```toml
[curiosity]
enabled = true
min_idle_seconds = 300  # 5 minutes before first ideation
categories = ["Exploration", "Optimization", "Security"]

# LLM-driven ideation (optional)
openai_api_key = "sk-..."
openai_base_url = "https://api.openai.com/v1"
model = "gpt-4o-mini"
```

### Metabolic Thresholds

```toml
[metabolic]
total_budget = 100000
warning_threshold = 0.8   # 80% usage
critical_threshold = 0.95  # 95% usage
```

---

## Testing

### Integration Tests

All six systems have comprehensive integration test coverage:

```bash
# System I: Ego-Matrix (12 tests)
cargo test --package zaion-ego --test integration

# System II: Autonomic Reflexes (21 tests)
cargo test --package zaion-autonomic --test integration

# System III: Hardware Proprioception (23 tests)
cargo test --package zaion-proprioception --test integration

# System IV: Metabolic Engine (30 tests)
cargo test --package zaion-metabolic --test integration

# System V: Entropic Curiosity (22 tests)
cargo test --package zaion-curiosity --test integration

# Integration Layer (18 tests)
cargo test --package zaion-singularity --test integration
```

**Total: 126 integration tests**

### Doctor Commands

Run health checks on each system:

```bash
zaion ego doctor
zaion autonomic doctor
zaion curiosity doctor
zaion proprioception doctor
zaion metabolic doctor
zaion singularity doctor
```

---

## Observability

### Runtime Metrics

Monitor system state in real-time:

```bash
# Gateway dashboard
zaion gateway start
# Open http://localhost:7821/console.html

# TUI dashboard
zaion dashboard
```

**Dashboard shows:**
- System I: Current ego.toml, Soul_Hash status
- System II: Active reflexes, ActionPotential levels
- System III: Environment fingerprint, shock severity
- System IV: Token budget graph, hunger level, pain signals
- System V: Idle timer, recent ideation prompts

### Event Ledger

All system events are signed and stored in the ledger:

```bash
# View recent events
zaion ledger show <pid> --last 50

# Query specific system events
zaion ledger query <pid> --system "System_V" --type "IdeationTriggered"

# Verify event signatures
zaion ledger verify <pid> --all
```

---

## Troubleshooting

### "Proactive prompts not triggering"

1. Check `proactive_rate` in `ego.toml` (must be > 0.0)
2. Verify System V is enabled: `zaion curiosity status <pid>`
3. Ensure sufficient idle time: `min_idle_seconds` default is 300s
4. Run doctor check: `zaion curiosity doctor`

### "Agent stuck in lockdown"

1. Check shock severity: `zaion proprioception status`
2. View detected differences: `zaion proprioception check`
3. If false positive, unlock: `zaion proprioception unlock <token>`
4. To disable lockdown entirely, set threshold to 0.0 in config

### "High token consumption, degraded responses"

1. Check budget: `zaion metabolic budget <pid>`
2. View hunger level: `zaion metabolic hunger <pid>`
3. Feed tokens to recover: `zaion metabolic feed <pid> 50000`
4. Adjust budget: `zaion metabolic set-budget <pid> 200000`

### "Reflexes not firing"

1. Verify autonomic runtime is active: `zaion autonomic status <pid>`
2. Check reflex count: `zaion autonomic list <pid>`
3. Start polling loop: `zaion autonomic start <pid>`
4. Run health check: `zaion autonomic doctor`

---

## Roadmap

### Current Status (2026-06-05)

- ✅ Systems I-V: 126 integration tests passing
- ✅ Doctor commands: All three systems (ego, autonomic, curiosity)
- ✅ Documentation: PROACTIVE_BEHAVIOR.md complete
- 🚧 TUI dashboard: Partial (memory, watchdog panes done)
- 🚧 Gateway integration: Proactive prompt streaming
- ⏳ Federation: Multi-agent proactive coordination

### Next Milestones

**Week 7-8: TUI Enhancement**
- Agentic loop visualization
- Real-time system metrics
- Interactive configuration editor

**Week 9-10: Advanced Features**
- Preference learning system
- Code self-modification
- Proactive tool suggestions

**Week 11-12: Production Readiness**
- Performance optimization
- Stress testing (1M+ token sessions)
- Security audit
- Public beta release

---

## FAQ

**Q: How does proactive mode differ from "agent loops" in other systems?**

A: Traditional agent loops are reactive: they wait for a user prompt, execute tools, and return a response. Zaion's proactive mode is **event-driven**: idle periods, token exhaustion, environment changes, and reflex triggers can all initiate autonomous behavior without user input.

**Q: Can I disable proactive behavior?**

A: Yes. Set `proactive_rate = 0.0` in `ego.toml` and `enabled = false` in `[curiosity]` config.

**Q: What happens if two systems conflict?**

A: The Singularity Runtime enforces a priority hierarchy:
1. System III (Proprioception) — can lock down all systems
2. System IV (Metabolic) — can throttle Systems II and V
3. System I (Ego) — can filter output from all systems
4. Systems II and V operate independently unless overridden

**Q: How much overhead does this add?**

A: Negligible. Systems I, III, and IV are synchronous checks (<1ms each). System II polls every 1000ms in the background. System V only activates after 5 minutes of idleness.

**Q: Can I extend these systems with custom logic?**

A: Yes:
- System I: Edit `ego.toml` directly
- System II: Register custom reflexes via API
- System III: Provide custom fingerprint collectors
- System IV: Register custom pain receptors
- System V: Add custom ideation categories

---

## See Also

- [MEMORY_SYSTEM.md](./MEMORY_SYSTEM.md) — Typed memory and auto-extraction
- [SELF_HEALING.md](./SELF_HEALING.md) — Watchdog Ouroboros protocol
- [GATEWAY.md](./GATEWAY.md) — WebSocket dashboard and streaming
- [FEATURE_AUDIT_2026_06_03.md](./FEATURE_AUDIT_2026_06_03.md) — Complete feature list

---

**Zaion v5.0 | Systems I-V Architecture**  
Copyright © 2025-2026 | MIT License
