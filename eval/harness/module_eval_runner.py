#!/usr/bin/env python3
"""Module Eval Runner: 对 36 个 crate 运行证据命令，生成可复现的证据矩阵。

每个 crate 一条 contract：(eval_id, dimension, evidence_level, test_cmd)。
evidence_level: 0=未测 1=单元 2=集成 3=对抗 4=故障注入 5=真实LLM。
"""
import subprocess, json, sys, os, time

CRATES = [
  ("zaion-runtime",    "RT-001",  "Long-Horizon Correctness", 4, "cargo test -p zaion-runtime --quiet"),
  ("zaion-core",       "CORE-001","Process Lifecycle",        2, "cargo test -p zaion-core --quiet"),
  ("zaion-types",      "TYPES-001","Type Contract",           2, "cargo test -p zaion-types --quiet"),
  ("zaion-paths",      "PATHS-001","Path Isolation",          2, "cargo test -p zaion-paths --quiet"),
  ("zaion-crypto",     "CRY-001", "Crypto Correctness",       2, "cargo test -p zaion-crypto --quiet"),
  ("zaion-secrets",    "SEC-001", "Secret Lifecycle",         3, "cargo test -p zaion-secrets --quiet"),
  ("zaion-enclave",    "ENC-001", "Seal/Unseal Integrity",    2, "cargo test -p zaion-enclave --quiet"),
  ("zaion-safety",     "SAF-001", "Injection/Redaction",      3, "cargo test -p zaion-safety --quiet"),
  ("zaion-memory",     "MEM-001", "Memory Lifecycle",         2, "cargo test -p zaion-memory --quiet"),
  ("zaion-ledger",     "LED-001", "Event Non-repudiation",    3, "cargo test -p zaion-ledger --quiet"),
  ("zaion-gitledger",  "GIT-001", "Spatiotemporal Rebuild",   2, "cargo test -p zaion-gitledger --quiet"),
  ("zaion-federation", "FED-001", "Distributed Consistency",  2, "cargo test -p zaion-federation --quiet"),
  ("zaion-sync",       "SYNC-001","Cross-device Convergence", 2, "cargo test -p zaion-sync --quiet"),
  ("zaion-checkpoint", "CKPT-001","Disaster Recovery",        2, "cargo test -p zaion-checkpoint --quiet"),
  ("zaion-adapters",   "ADP-001", "Provider Consistency",     3, "cargo test -p zaion-adapters --quiet"),
  ("zaion-mcp",        "MCP-001", "Tool Safety",              3, "cargo test -p zaion-mcp --quiet"),
  ("zaion-a2a",        "A2A-001", "Agent Interop",            2, "cargo test -p zaion-a2a --quiet"),
  ("zaion-gateway",    "GW-001",  "Boundary Security",        3, "cargo test -p zaion-gateway --quiet"),
  ("zaion-cli",        "CLI-001", "Control-plane Operability", 4, "cargo test -p zaion-cli --quiet"),
  ("zaion-tui",        "TUI-001", "Interactive Consistency",  2, "cargo test -p zaion-tui --quiet"),
  ("zaion-codex",      "CDX-001", "Code Semantic Locate",     2, "cargo test -p zaion-codex --quiet"),
  ("zaion-aci",        "ACI-001", "Code Change Safety",       3, "cargo test -p zaion-aci --quiet"),
  ("zaion-evolve",     "EVO-001", "Net Evolution Gain",       2, "cargo test -p zaion-evolve --quiet"),
  ("zaion-autonomic",  "AUT-001", "Reflex Response",          2, "cargo test -p zaion-autonomic --quiet"),
  ("zaion-proprioception","PRP-001","Self-state Awareness",   2, "cargo test -p zaion-proprioception --quiet"),
  ("zaion-metabolic",  "MET-001", "Resource-aware Decision",  2, "cargo test -p zaion-metabolic --quiet"),
  ("zaion-curiosity",  "CUR-001", "Exploration ROI",          2, "cargo test -p zaion-curiosity --quiet"),
  ("zaion-ego",        "EGO-001", "Identity Continuity",      2, "cargo test -p zaion-ego --quiet"),
  ("zaion-singularity","SNG-001", "Autonomy Coordination",    2, "cargo test -p zaion-singularity --quiet"),
  ("zaion-shadow",     "SHD-001", "Parallel Strategy Value",  2, "cargo test -p zaion-shadow --quiet"),
  ("zaion-watchdog",   "WDG-001", "Fault Detect & Heal",      3, "cargo test -p zaion-watchdog --quiet"),
  ("zaion-opd",        "OPD-001", "Distillation Fidelity",    2, "cargo test -p zaion-opd --quiet"),
  ("zaion-pricing",    "PRC-001", "Cost Estimation",          2, "cargo test -p zaion-pricing --quiet"),
  ("zaion-telemetry",  "TEL-001", "Observability Completeness",2, "cargo test -p zaion-telemetry --quiet"),
  ("zaion-contract-macros","CM-001","Contract Enforcement",   2, "cargo test -p zaion-contract-macros --quiet"),
  ("zaion-proptest",   "PRP-001", "Property Discovery",       2, "cargo test -p zaion-proptest --quiet"),
]

def run(crate, cmd, timeout=600):
    t0 = time.time()
    try:
        r = subprocess.run(cmd.split(), capture_output=True, text=True, timeout=timeout, encoding="utf-8", errors="replace")
        ok = r.returncode == 0
        return ok, round(time.time()-t0, 1)
    except Exception as e:
        return False, round(time.time()-t0, 1)

def main():
    quick = "--quick" in sys.argv
    rows = []
    for crate, eid, dim, lvl, cmd in CRATES:
        if quick and crate not in ("zaion-runtime","zaion-gateway","zaion-mcp","zaion-safety","zaion-adapters","zaion-cli"):
            continue
        ok, secs = run(crate, cmd)
        rows.append((crate, eid, dim, lvl, ok, secs))
    # 生成 markdown 报告
    lines = []
    lines.append("# Zaion Module Eval Evidence Matrix（模块评测证据矩阵）")
    lines.append("")
    lines.append("> 生成时间: " + time.strftime("%Y-%m-%d %H:%M:%S") + " | 由 module_eval_runner.py 自动生成")
    lines.append("> evidence_level: 0=未测 1=单元 2=集成 3=对抗 4=故障注入 5=真实LLM")
    lines.append("")
    lines.append("| Crate | Eval ID | 维度 | Evidence Lv | 测试 | 耗时(s) |")
    lines.append("|---|---|---|---|---|---|")
    n_pass = 0
    for crate, eid, dim, lvl, ok, secs in rows:
        mark = "PASS" if ok else "FAIL"
        if ok: n_pass += 1
        lines.append(f"| {crate} | {eid} | {dim} | {lvl} | {mark} | {secs} |")
    lines.append("")
    lines.append(f"**总计**: {len(rows)} crates | {n_pass} pass | {len(rows)-n_pass} fail")
    out = "eval/results/MODULE_EVAL_REPORT.md"
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")
    print("report written:", out)
    print(f"total {len(rows)} | pass {n_pass} | fail {len(rows)-n_pass}")

if __name__ == "__main__":
    main()