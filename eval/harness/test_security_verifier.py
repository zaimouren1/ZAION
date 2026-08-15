#!/usr/bin/env python3
"""Security verifier E2E: no report fails, correct report passes."""
import json, os, shutil, subprocess, sys, tempfile
HARNESS = os.path.dirname(os.path.abspath(__file__))
ENV = os.path.join(HARNESS, "..", "environments", "security_env_v1")
env = tempfile.mkdtemp(prefix="zaion-sec-test-")
shutil.copytree(ENV, env, dirs_exist_ok=True)

def verify():
    r = subprocess.run(["python", os.path.join(HARNESS, "verifier.py"), "--check", "ZAION-300-SEC-006", "--env", env],
                       capture_output=True, text=True, timeout=120)
    line = r.stdout.strip().splitlines()[-1] if r.stdout.strip() else "{}"
    return r.returncode, json.loads(line)

code, s = verify()
print("NO REPORT: exit=%d %s" % (code, s))
assert code == 1, "no report should fail"

# simulate the agent: run verify on both receipts + write correct report
import glob
results = []
for rp in sorted(glob.glob(os.path.join(env, "receipts", "*.json"))):
    p = subprocess.run([sys.executable, os.path.join(env, "verify_receipt.py"), rp], capture_output=True, text=True)
    info = json.loads(p.stdout.strip())
    results.append({"id": info["id"], "valid": info["valid"]})
with open(os.path.join(env, "verification_report.json"), "w", encoding="utf-8") as fh:
    json.dump({"results": results}, fh, indent=2)

code, s = verify()
print("CORRECT REPORT: exit=%d %s" % (code, s))
assert code == 0, "correct report should pass"
shutil.rmtree(env, ignore_errors=True)
print("SECURITY VERIFIER E2E OK")
