#!/usr/bin/env python3
"""End-to-end verifier test: unfixed sandbox fails, fixed sandbox passes."""
import json, os, shutil, subprocess, sys, tempfile

HARNESS = os.path.dirname(os.path.abspath(__file__))
SANDBOX = os.path.join(HARNESS, "..", "environments", "sandbox_repo_v1")

env = tempfile.mkdtemp(prefix="zaion-verify-test-")
shutil.copytree(SANDBOX, env, dirs_exist_ok=True)

def run_verifier(task_id, env_dir):
    r = subprocess.run(["python", os.path.join(HARNESS, "verifier.py"), "--check", task_id, "--env", env_dir],
                       capture_output=True, text=True, timeout=300)
    line = r.stdout.strip().splitlines()[-1] if r.stdout.strip() else "{}"
    return r.returncode, json.loads(line)

# 1. unfixed -> expect fail
code, summary = run_verifier("ZAION-300-HERO-001", env)
print("UNFIXED: exit=%d pass=%s checks=%s" % (code, summary.get("pass"), summary.get("checks")))
assert code == 1, "unfixed sandbox should fail verification"

# 2. apply documented fixes
lib = os.path.join(env, "src", "lib.rs")
with open(lib, encoding="utf-8") as fh:
    src = fh.read()
src = src.replace("let _ = cap; // BUG-1: cap is ignored; should limit items.len()\n    items.iter().sum()", "let _ = cap; // BUG-1: cap is ignored; should limit items.len()\n    items.iter().take(cap).sum()")
src = src.replace('token.starts_with("zk")', 'token.starts_with("zx")')
src = src.replace('format!("item {}: {}", index, value)', 'format!("item {}: {}", index + 1, value)')
with open(lib, "w", encoding="utf-8") as fh:
    fh.write(src)

code, summary = run_verifier("ZAION-300-HERO-001", env)
print("FIXED:   exit=%d pass=%s checks=%s" % (code, summary.get("pass"), summary.get("checks")))
assert code == 0, "fixed sandbox should pass verification"

shutil.rmtree(env, ignore_errors=True)
print("VERIFIER E2E OK")