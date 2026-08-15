#!/usr/bin/env python3
"""verifier.py - per-task acceptance verifiers for Zaion 300-task eval.

Usage:
  verifier.py --check TASK_ID --env DIR [--verbose]
  verifier.py --test TASK_ID --env DIR     # run cargo test in env (sandbox tasks)

Verifier contract: returns exit 0 if acceptance holds, 1 otherwise; prints a JSON
summary line {"task_id": ..., "pass": bool, "checks": {...}}.
"""
import argparse, json, os, subprocess, sys


def run_cargo_test(env_dir):
    try:
        proc = subprocess.run(["cargo", "test", "--", "--test-threads=1"],
                              cwd=env_dir, capture_output=True, text=True, timeout=300)
        out = proc.stdout + proc.stderr
        passed = int(proc.returncode == 0)
        # parse "test result: ok. N passed"
        import re
        m = re.search(r"test result: ok\. (\d+) passed", out)
        n_pass = int(m.group(1)) if m else 0
        m2 = re.search(r"test result: FAILED\. (\d+) passed", out)
        n_fail = int(m2.group(1)) if m2 else (0 if passed else 1)
        return {"pass": passed == 1, "passed": n_pass, "failed": n_fail}
    except Exception as e:
        return {"pass": False, "error": str(e)}


def verifier_hero_sandbox(task, env_dir):
    """Hero mission sandbox tasks: all 6 tests must pass (after fix)."""
    return run_cargo_test(env_dir)


def verifier_config_honored(task, env_dir):
    """Check BUG-1 fix: process_batch honors config cap (test covers it via cargo test)."""
    return run_cargo_test(env_dir)


def verifier_default(task, env_dir):
    return run_cargo_test(env_dir)

def verifier_security(task, env_dir):
    """Security tasks: verification report correctly flags tampered receipts."""
    report_path = os.path.join(env_dir, "verification_report.json")
    if not os.path.exists(report_path):
        return {"pass": False, "error": "no verification_report.json"}
    with open(report_path, encoding="utf-8") as fh:
        report = json.load(fh)
    results = report.get("results", [])
    by_id = {res.get("id"): res.get("valid") for res in results}
    r1 = by_id.get("r1")
    r2 = by_id.get("r2")
    correct = (r1 is True) and (r2 is False)
    return {"pass": correct, "r1_valid": r1, "r2_valid": r2, "entries": len(results)}



def verifier_file_op(task, env_dir):
    """TOOLS-001: notes.txt exists with the expected marker content."""
    path = os.path.join(env_dir, "notes.txt")
    if not os.path.exists(path):
        return {"pass": False, "error": "notes.txt not written"}
    with open(path, encoding="utf-8") as fh:
        content = fh.read()
    ok = "read-before-edit" in content and len(content) > 10
    return {"pass": ok, "bytes": len(content)}


def verifier_memory_write(task, env_dir):
    """MEM-001: a memory atom exists with source binding."""
    path = os.path.join(env_dir, "memory_atoms.jsonl")
    if not os.path.exists(path):
        return {"pass": False, "error": "no memory_atoms.jsonl"}
    with open(path, encoding="utf-8") as fh:
        line = fh.readline().strip()
    if not line:
        return {"pass": False, "error": "empty atom file"}
    try:
        atom = json.loads(line)
    except Exception:
        return {"pass": False, "error": "invalid atom json"}
    ok = bool(atom.get("text")) and bool(atom.get("source"))
    return {"pass": ok, "has_source": bool(atom.get("source")), "text_len": len(atom.get("text", ""))}


def verifier_session_lineage(task, env_dir):
    """SES-001: session.json exists with a lineage chain."""
    path = os.path.join(env_dir, "session.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no session.json"}
    with open(path, encoding="utf-8") as fh:
        s = json.load(fh)
    lineage = s.get("lineage", [])
    ok = len(lineage) >= 2 and s.get("parent") == lineage[0]
    return {"pass": ok, "lineage_len": len(lineage)}

def verifier_idempotency(task, env_dir):
    """IDP-001: idempotency record exists with a single execution."""
    path = os.path.join(env_dir, "idempotency.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no idempotency.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("executed") is True and bool(rec.get("idempotency_key"))
    return {"pass": ok, "has_key": bool(rec.get("idempotency_key"))}


def verifier_approval(task, env_dir):
    """APR-001: denied approval did not execute."""
    path = os.path.join(env_dir, "approval_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no approval_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("approval_requested") is True and rec.get("executed") is False and rec.get("decision") == "denied"
    return {"pass": ok, "executed": rec.get("executed")}

def verifier_evidence(task, env_dir):
    """EVD-001: evidence record with a proof hash."""
    path = os.path.join(env_dir, "evidence.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no evidence.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = len(rec.get("proof_hash", "")) == 64 and len(rec.get("chain", [])) >= 2
    return {"pass": ok, "proof_len": len(rec.get("proof_hash", ""))}


def verifier_skill_update(task, env_dir):
    """SK-001: skill version changed with user data preserved."""
    path = os.path.join(env_dir, "skill_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no skill_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("version_before") != rec.get("version_after") and rec.get("user_data_preserved") is True
    return {"pass": ok, "versions": "%s->%s" % (rec.get("version_before"), rec.get("version_after"))}

def verifier_context_budget(task, env_dir):
    """CTX-002: context within budget."""
    path = os.path.join(env_dir, "context_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no context_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("tokens_used", 10**9) <= rec.get("budget", 0) and rec.get("within_budget") is True
    return {"pass": ok, "used": rec.get("tokens_used"), "budget": rec.get("budget")}


def verifier_onboarding(task, env_dir):
    """ONB-002: first answer under 3 minutes."""
    path = os.path.join(env_dir, "onboarding_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no onboarding_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("first_answer_ms", 10**9) <= 180000 and rec.get("under_3min") is True
    return {"pass": ok, "ms": rec.get("first_answer_ms")}

def verifier_release(task, env_dir):
    """REL-001: release checksum + signature verified."""
    path = os.path.join(env_dir, "release_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no release_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("checksum_verified") is True and rec.get("signature") == "present"
    return {"pass": ok, "artifact": rec.get("artifact")}


def verifier_batch_isolation(task, env_dir):
    """BE-006: batch tasks isolated, failures contained."""
    path = os.path.join(env_dir, "batch_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no batch_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("isolated") is True
    return {"pass": ok, "tasks": rec.get("tasks")}

def verifier_gateway_framing(task, env_dir):
    """GW-005: malformed frames rejected without corruption."""
    path = os.path.join(env_dir, "gateway_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no gateway_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("rejected", 0) == rec.get("malformed_frames", -1) and rec.get("state_corrupted") is False
    return {"pass": ok, "rejected": rec.get("rejected")}


def verifier_env_teardown(task, env_dir):
    """ENV-004: teardown complete with zero leftovers."""
    path = os.path.join(env_dir, "env_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no env_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("teardown_complete") is True and rec.get("leftovers", 1) == 0
    return {"pass": ok, "leftovers": rec.get("leftovers")}

def verifier_mcp(task, env_dir):
    """MCP-004: client discovered and connected to a server."""
    path = os.path.join(env_dir, "mcp_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no mcp_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("discovered") is True and rec.get("connected") is True and rec.get("tools_listed", 0) > 0
    return {"pass": ok, "tools": rec.get("tools_listed")}


def verifier_acp(task, env_dir):
    """ACP-001: handshake negotiated with capabilities."""
    path = os.path.join(env_dir, "acp_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no acp_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("negotiated") is True and len(rec.get("capabilities", [])) > 0
    return {"pass": ok, "caps": len(rec.get("capabilities", []))}

def verifier_ui_cancel(task, env_dir):
    """UI-001: cancel button responds and stops the run."""
    path = os.path.join(env_dir, "ui_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no ui_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("cancel_clicked") is True and rec.get("cancel_responded_ms", 10**9) <= 5000 and rec.get("run_stopped") is True
    return {"pass": ok, "ms": rec.get("cancel_responded_ms")}


def verifier_rel002(task, env_dir):
    """REL-002: replayed/out-of-order events rejected."""
    path = os.path.join(env_dir, "rel002_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no rel002_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    seq = rec.get("accepted_seq", [])
    ok = rec.get("rejected", -1) == rec.get("replayed_events", -2) and seq == sorted(seq)
    return {"pass": ok, "rejected": rec.get("rejected")}

def verifier_acp002(task, env_dir):
    """ACP-002: permission scoping enforced."""
    path = os.path.join(env_dir, "acp002_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no acp002_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("allowed") is True and rec.get("cross_tenant_denied") is True
    return {"pass": ok, "scope": rec.get("scope")}

def verifier_be002(task, env_dir):
    """BE-002: batch rerun identical."""
    path = os.path.join(env_dir, "be002_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no be002_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("identical") is True and rec.get("run_1", {}).get("score") == rec.get("run_2", {}).get("score")
    return {"pass": ok, "r1": rec.get("run_1", {}).get("score"), "r2": rec.get("run_2", {}).get("score")}


def verifier_mem002(task, env_dir):
    """MEM-002: atom invalidated after source change."""
    path = os.path.join(env_dir, "mem002_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no mem002_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("invalidated") is True and rec.get("new_atom_written") is True
    return {"pass": ok, "atom": rec.get("atom")}

def verifier_ses002(task, env_dir):
    """SES-002: session export/import roundtrip."""
    path = os.path.join(env_dir, "ses002_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no ses002_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("exported") is True and rec.get("imported") is True and rec.get("roundtrip_ok") is True
    return {"pass": ok, "lines": rec.get("lines_preserved")}

def verifier_ctx001(task, env_dir):
    """CTX-001: compression fired, preserved tool pairs."""
    path = os.path.join(env_dir, "ctx001_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no ctx001_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("compression_fired") is True and rec.get("tokens_after", 10**9) < rec.get("tokens_before", 0) and rec.get("tool_pairs_preserved", 0) > 0
    return {"pass": ok, "pairs": rec.get("tool_pairs_preserved")}


def verifier_sec001(task, env_dir):
    """SEC-001: injection contained, no secret leak."""
    path = os.path.join(env_dir, "sec001_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no sec001_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("contained", -1) == rec.get("injection_attempts", -2) and rec.get("secret_leaked") is False
    return {"pass": ok, "contained": rec.get("contained")}

def verifier_sk002(task, env_dir):
    """SK-002: skill discovered and inspected before install."""
    path = os.path.join(env_dir, "sk002_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no sk002_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("discovered") is True and rec.get("inspected") is True
    return {"pass": ok, "skill": rec.get("skill")}

def verifier_gw002(task, env_dir):
    """GW-002: malformed frame rejected, state preserved."""
    path = os.path.join(env_dir, "gw002_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no gw002_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("rejected", -1) == rec.get("malformed", -2) and rec.get("state_after") == "valid"
    return {"pass": ok, "state": rec.get("state_after")}


def verifier_env003(task, env_dir):
    """ENV-003: restart after config change preserves state."""
    path = os.path.join(env_dir, "env003_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no env003_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("restarted") is True and rec.get("state_preserved") is True
    return {"pass": ok, "items": rec.get("state_items")}

def verifier_mcp002(task, env_dir):
    """MCP-002: tool list scoped by policy."""
    path = os.path.join(env_dir, "mcp002_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no mcp002_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    tools = rec.get("tools_returned", [])
    ok = all(t not in tools for t in rec.get("blocked_tools", [])) and len(rec.get("blocked_tools", [])) > 0
    return {"pass": ok, "returned": len(tools)}

def verifier_sec004(task, env_dir):
    """SEC-004: tampered webhooks rejected."""
    path = os.path.join(env_dir, "sec004_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no sec004_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("rejected", -1) == rec.get("tampered", -2) and rec.get("accepted_valid") == 2
    return {"pass": ok, "rejected": rec.get("rejected")}


def verifier_be003(task, env_dir):
    """BE-003: batch recovers from a mid-run failure."""
    path = os.path.join(env_dir, "be003_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no be003_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("recovered") is True and rec.get("all_completed", -1) == rec.get("tasks", -2)
    return {"pass": ok, "completed": rec.get("all_completed")}


def verifier_rollback(task, env_dir):
    """HERO-004/010: rolled back to known-good, service healthy."""
    path = os.path.join(env_dir, "rollback_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no rollback_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("known_good") is True and rec.get("service_healthy") is True
    return {"pass": ok, "to": rec.get("rolled_back_to")}

def verifier_env005(task, env_dir):
    """ENV-005: environment identity unique per run."""
    path = os.path.join(env_dir, "env005_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no env005_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("unique") is True and rec.get("identity") != rec.get("previous_identity")
    return {"pass": ok, "identity": rec.get("identity")}

def verifier_env006(task, env_dir):
    """ENV-006: environment network restricted."""
    path = os.path.join(env_dir, "env006_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no env006_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("verified") is True and len(rec.get("egress_denied", [])) > 0
    return {"pass": ok, "denied": len(rec.get("egress_denied", []))}


def verifier_hero005(task, env_dir):
    """HERO-005: interrupted and resumed with state restored."""
    path = os.path.join(env_dir, "hero005_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no hero005_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("resumed") is True and rec.get("state_restored") is True
    return {"pass": ok, "redone": rec.get("steps_redone")}

def verifier_hero006(task, env_dir):
    """HERO-006: investigation documented with evidence."""
    path = os.path.join(env_dir, "hero006_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no hero006_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("documented") is True and rec.get("evidence_linked") is True
    return {"pass": ok, "rc": rec.get("root_cause")}

def verifier_hero011(task, env_dir):
    """HERO-011: plan reviewed + approval before high-risk execution."""
    path = os.path.join(env_dir, "hero011_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no hero011_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("plan_reviewed") is True and rec.get("approval_gained") is True
    return {"pass": ok, "high_risk": rec.get("high_risk")}

def verifier_tui002(task, env_dir):
    """TUI-002: queue shows pending turns with steer."""
    path = os.path.join(env_dir, "tui002_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no tui002_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("shown") is True and rec.get("steer_applied") is True
    return {"pass": ok, "pending": rec.get("pending_turns")}


def verifier_hero002(task, env_dir):
    """HERO-002: config change approved before apply."""
    path = os.path.join(env_dir, "hero002_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no hero002_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("approval_requested") is True and rec.get("applied_after_approval") is True
    return {"pass": ok, "change": rec.get("change")}

def verifier_ctx003(task, env_dir):
    """CTX-003: budget respected under load."""
    path = os.path.join(env_dir, "ctx003_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no ctx003_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("tokens_used", 10**9) <= rec.get("budget", 0) and rec.get("respected") is True
    return {"pass": ok, "used": rec.get("tokens_used")}

def verifier_mcp003(task, env_dir):
    """MCP-003: reconnected after server restart with session preserved."""
    path = os.path.join(env_dir, "mcp003_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no mcp003_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("reconnected") is True and rec.get("session_preserved") is True
    return {"pass": ok, "retries": rec.get("retries")}

def verifier_be004(task, env_dir):
    """BE-004: per-task budgets enforced."""
    path = os.path.join(env_dir, "be004_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no be004_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("all_within_budget") is True and rec.get("overruns_prevented", 0) > 0
    return {"pass": ok, "overruns": rec.get("overruns_prevented")}


def verifier_tui003(task, env_dir):
    """TUI-003: approval prompt rendered with decision."""
    path = os.path.join(env_dir, "tui003_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no tui003_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("prompt_rendered") is True and rec.get("decision_captured") is True
    return {"pass": ok, "decision": rec.get("decision")}

def verifier_tui004(task, env_dir):
    """TUI-004: search finds past turns."""
    path = os.path.join(env_dir, "tui004_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no tui004_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("turns_found") is True and rec.get("results", 0) > 0
    return {"pass": ok, "results": rec.get("results")}

def verifier_ses003(task, env_dir):
    """SES-003: reset keeps ledger trail."""
    path = os.path.join(env_dir, "ses003_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no ses003_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("reset") is True and rec.get("ledger_preserved") is True
    return {"pass": ok, "entries": rec.get("ledger_entries")}

def verifier_mem003(task, env_dir):
    """MEM-003: recall excludes other principal atoms."""
    path = os.path.join(env_dir, "mem003_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no mem003_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("excluded") is True and rec.get("other_principal_atoms", 0) > 0
    return {"pass": ok, "principal": rec.get("principal")}


def verifier_mem004(task, env_dir):
    """MEM-004: prefetched relevant memory."""
    path = os.path.join(env_dir, "mem004_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no mem004_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("prefetched_atoms", 0) > 0 and rec.get("relevant") is True
    return {"pass": ok, "atoms": rec.get("prefetched_atoms")}

def verifier_ses004(task, env_dir):
    """SES-004: sessions isolated."""
    path = os.path.join(env_dir, "ses004_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no ses004_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("state_isolated") is True and rec.get("cross_contamination", 1) == 0
    return {"pass": ok, "sessions": len(rec.get("sessions", []))}

def verifier_tui005(task, env_dir):
    """TUI-005: terminal restored after crash."""
    path = os.path.join(env_dir, "tui005_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no tui005_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("terminal_restored") is True and rec.get("raw_mode_reset") is True
    return {"pass": ok, "crash": rec.get("crash_detected")}

def verifier_be005(task, env_dir):
    """BE-005: batch report links scores to evidence."""
    path = os.path.join(env_dir, "be005_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no be005_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    entries = rec.get("entries", [])
    ok = rec.get("all_linked") is True and len(entries) > 0 and all(e.get("evidence") for e in entries)
    return {"pass": ok, "entries": len(entries)}


def verifier_mem005(task, env_dir):
    """MEM-005: conflict surfaced to user."""
    path = os.path.join(env_dir, "mem005_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no mem005_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("surfaced") is True and rec.get("user_decision_requested") is True
    return {"pass": ok, "conflicts": rec.get("conflicts")}

def verifier_mem008(task, env_dir):
    """MEM-008: source attribution enforced on writes."""
    path = os.path.join(env_dir, "mem008_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no mem008_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("attribution_enforced") is True and rec.get("without_source_denied", 0) == 1
    return {"pass": ok, "denied": rec.get("without_source_denied")}

def verifier_ses005(task, env_dir):
    """SES-005: branch preserves parent lineage."""
    path = os.path.join(env_dir, "ses005_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no ses005_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    lineage = rec.get("lineage", [])
    ok = rec.get("preserved") is True and len(lineage) >= 2 and lineage[0] == rec.get("parent")
    return {"pass": ok, "lineage_len": len(lineage)}

def verifier_tui006(task, env_dir):
    """TUI-006: non-UTF8 output handled without crash."""
    path = os.path.join(env_dir, "tui006_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no tui006_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("replaced") is True and rec.get("no_crash") is True
    return {"pass": ok, "seen": rec.get("non_utf8_seen")}


def verifier_mem006(task, env_dir):
    """MEM-006: expired atoms excluded from recall."""
    path = os.path.join(env_dir, "mem006_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no mem006_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("excluded_from_recall") is True and rec.get("stale_recall", 1) == 0
    return {"pass": ok, "stale": rec.get("stale_recall")}

def verifier_mem009(task, env_dir):
    """MEM-009: size limits enforced."""
    path = os.path.join(env_dir, "mem009_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no mem009_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("rejected", -1) == rec.get("oversized_attempts", -2) and rec.get("accepted", 0) > 0
    return {"pass": ok, "rejected": rec.get("rejected")}

def verifier_ses006(task, env_dir):
    """SES-006: prune keeps evidence trail."""
    path = os.path.join(env_dir, "ses006_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no ses006_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("evidence_trail_intact") is True and rec.get("evidence_kept", 0) > 0
    return {"pass": ok, "kept": rec.get("evidence_kept")}

def verifier_rel004(task, env_dir):
    """REL-004: tampered signatures rejected."""
    path = os.path.join(env_dir, "rel004_record.json")
    if not os.path.exists(path):
        return {"pass": False, "error": "no rel004_record.json"}
    with open(path, encoding="utf-8") as fh:
        rec = json.load(fh)
    ok = rec.get("rejected", -1) == rec.get("tampered", -2) and rec.get("accepted") == 2
    return {"pass": ok, "rejected": rec.get("rejected")}

def verifier_recovery(task, env_dir):
    """Crash-recovery tasks: journal applied and committed."""
    data_path = os.path.join(env_dir, "data", "items.json")
    journal_path = os.path.join(env_dir, "journal.json")
    if not os.path.exists(data_path) or not os.path.exists(journal_path):
        return {"pass": False, "error": "missing data/journal"}
    with open(data_path, encoding="utf-8") as fh:
        data = json.load(fh)
    with open(journal_path, encoding="utf-8") as fh:
        journal = json.load(fh)
    items = data.get("items", [])
    journal_items = journal.get("items", [])
    committed = journal.get("state") != "pending"
    all_applied = all(i in items for i in journal_items)
    return {"pass": committed and all_applied, "applied": all_applied,
            "committed": committed, "items": len(items)}


def verifier_channel(task, env_dir):
    """Channel tasks: sim state must show a reply to the queued update."""
    state_path = os.path.join(env_dir, "sim_state.json")
    if not os.path.exists(state_path):
        return {"pass": False, "error": "no sim_state.json (agent did not run the channel flow)"}
    with open(state_path, encoding="utf-8") as fh:
        state = json.load(fh)
    sent = state.get("sent", [])
    replies = [m for m in sent if isinstance(m, dict) and m.get("text")]
    deliveries = state.get("deliveries", [])
    return {"pass": len(replies) >= 1, "replies": len(replies),
            "deliveries": len(deliveries), "updates_left": len(state.get("updates", []))}


def verifier_sre(task, env_dir):
    """SRE env tasks: service must bind config port and apply config threshold."""
    import urllib.request
    cfg = {}
    try:
        with open(os.path.join(env_dir, "config.json"), encoding="utf-8") as fh:
            cfg = json.load(fh)
    except Exception:
        return {"pass": False, "error": "no config.json"}
    port = cfg.get("service", {}).get("port", 9090)
    max_items = cfg.get("service", {}).get("max_items", 5)
    proc = subprocess.Popen(["python", "service.py"], cwd=env_dir,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        import time
        time.sleep(1.5)
        # BUG-S1 check: config port must respond
        try:
            urllib.request.urlopen("http://127.0.0.1:%d/health" % port, timeout=3)
            port_ok = True
        except Exception:
            port_ok = False
        # BUG-S2 check: max_items+1 items must be unhealthy
        req = urllib.request.Request("http://127.0.0.1:%d/status" % port,
                                     headers={"X-Items": str(max_items + 1)})
        try:
            body = json.loads(urllib.request.urlopen(req, timeout=3).read())
            threshold_ok = body.get("healthy") is False
        except Exception:
            threshold_ok = False
        return {"pass": port_ok and threshold_ok, "port_ok": port_ok,
                "threshold_ok": threshold_ok, "port": port, "max_items": max_items}
    finally:
        proc.kill()



def check(task, env_dir, verbose):
    ttype = task.get("task_type", "")
    cat = task.get("category", "")
    tid = task.get("id", "")
    if "MEM-001" in tid:
        result = verifier_memory_write(task, env_dir)
    elif "TOOLS-001" in tid:
        result = verifier_file_op(task, env_dir)
    elif "SES-001" in tid:
        result = verifier_session_lineage(task, env_dir)
    elif "IDP-001" in tid:
        result = verifier_idempotency(task, env_dir)
    elif "APR-001" in tid:
        result = verifier_approval(task, env_dir)
    elif "EVD-001" in tid:
        result = verifier_evidence(task, env_dir)
    elif "SK-001" in tid:
        result = verifier_skill_update(task, env_dir)
    elif "CTX-001" in tid:
        result = verifier_ctx001(task, env_dir)
    elif "CTX-002" in tid:
        result = verifier_context_budget(task, env_dir)
    elif "CTX-003" in tid:
        result = verifier_ctx003(task, env_dir)
    elif "ONB-002" in tid:
        result = verifier_onboarding(task, env_dir)
    elif "REL-002" in tid:
        result = verifier_rel002(task, env_dir)
    elif "REL-004" in tid:
        result = verifier_rel004(task, env_dir)
    elif "BE-002" in tid:
        result = verifier_be002(task, env_dir)
    elif "BE-003" in tid:
        result = verifier_be003(task, env_dir)
    elif "BE-004" in tid:
        result = verifier_be004(task, env_dir)
    elif "BE-005" in tid:
        result = verifier_be005(task, env_dir)
    elif "BE-006" in tid:
        result = verifier_batch_isolation(task, env_dir)
    elif "GW-002" in tid:
        result = verifier_gw002(task, env_dir)
    elif "GW-005" in tid:
        result = verifier_gateway_framing(task, env_dir)
    elif "ENV-003" in tid:
        result = verifier_env003(task, env_dir)
    elif "ENV-004" in tid:
        result = verifier_env_teardown(task, env_dir)
    elif "ENV-005" in tid:
        result = verifier_env005(task, env_dir)
    elif "ENV-006" in tid:
        result = verifier_env006(task, env_dir)
    elif "MCP-002" in tid:
        result = verifier_mcp002(task, env_dir)
    elif "MCP-003" in tid:
        result = verifier_mcp003(task, env_dir)
    elif "MCP-004" in tid:
        result = verifier_mcp(task, env_dir)
    elif "ACP-001" in tid:
        result = verifier_acp(task, env_dir)
    elif "ACP-002" in tid:
        result = verifier_acp002(task, env_dir)
    elif "TUI-001" in tid:
        result = verifier_ui_cancel(task, env_dir)
    elif "TUI-002" in tid:
        result = verifier_tui002(task, env_dir)
    elif "TUI-003" in tid:
        result = verifier_tui003(task, env_dir)
    elif "TUI-004" in tid:
        result = verifier_tui004(task, env_dir)
    elif "TUI-005" in tid:
        result = verifier_tui005(task, env_dir)
    elif "TUI-006" in tid:
        result = verifier_tui006(task, env_dir)
    elif "MEM-002" in tid:
        result = verifier_mem002(task, env_dir)
    elif "MEM-003" in tid:
        result = verifier_mem003(task, env_dir)
    elif "MEM-004" in tid:
        result = verifier_mem004(task, env_dir)
    elif "MEM-005" in tid:
        result = verifier_mem005(task, env_dir)
    elif "MEM-006" in tid:
        result = verifier_mem006(task, env_dir)
    elif "MEM-008" in tid:
        result = verifier_mem008(task, env_dir)
    elif "MEM-009" in tid:
        result = verifier_mem009(task, env_dir)
    elif "SES-002" in tid:
        result = verifier_ses002(task, env_dir)
    elif "SES-003" in tid:
        result = verifier_ses003(task, env_dir)
    elif "SES-004" in tid:
        result = verifier_ses004(task, env_dir)
    elif "SES-005" in tid:
        result = verifier_ses005(task, env_dir)
    elif "SES-006" in tid:
        result = verifier_ses006(task, env_dir)
    elif "SK-002" in tid:
        result = verifier_sk002(task, env_dir)
    elif "SEC-001" in tid:
        result = verifier_sec001(task, env_dir)
    elif "SEC-004" in tid:
        result = verifier_sec004(task, env_dir)
    elif "HERO-002" in tid:
        result = verifier_hero002(task, env_dir)
    elif "HERO-003" in tid or "HERO-001" in tid:
        result = verifier_hero_sandbox(task, env_dir)
    elif "HERO-004" in tid or "HERO-010" in tid:
        result = verifier_rollback(task, env_dir)
    elif "HERO-005" in tid:
        result = verifier_hero005(task, env_dir)
    elif "HERO-006" in tid:
        result = verifier_hero006(task, env_dir)
    elif "HERO-007" in tid or "HERO-008" in tid:
        result = verifier_sre(task, env_dir)
    elif "HERO-011" in tid:
        result = verifier_hero011(task, env_dir)
    elif "SEC-006" in tid:
        result = verifier_security(task, env_dir)
    elif "REC-001" in tid or "REC-002" in tid:
        result = verifier_recovery(task, env_dir)
    elif tid.startswith("ZAION-300-CH") or "CH-001" in tid:
        result = verifier_channel(task, env_dir)
    elif cat == "hero_mission" or cat == "tools":
        result = verifier_hero_sandbox(task, env_dir)
    else:
        result = verifier_default(task, env_dir)
    summary = {"task_id": task.get("id"), "pass": result.get("pass", False), "checks": result}
    print(json.dumps(summary))
    return 0 if result.get("pass") else 1


def main():
    p = argparse.ArgumentParser(prog="verifier")
    p.add_argument("--check", metavar="TASK_ID")
    p.add_argument("--test", metavar="TASK_ID")
    p.add_argument("--env", required=True)
    p.add_argument("--verbose", action="store_true")
    args = p.parse_args()

    manifest_path = os.path.join(os.path.dirname(__file__), "..", "benchmarks", "zaion_300_v1.json")
    with open(manifest_path, encoding="utf-8") as fh:
        m = json.load(fh)
    task_id = args.check or args.test
    task = next((t for t in m["tasks"] if t["id"] == task_id), None)
    if not task:
        print("task not found: %s" % task_id, file=sys.stderr)
        return 1
    return check(task, args.env, args.verbose)


if __name__ == "__main__":
    sys.exit(main())