#!/usr/bin/env python3
"""Recovery verifier E2E: unfixed fails, recovered passes."""
import json, os, shutil, subprocess, sys, tempfile
HARNESS = os.path.dirname(os.path.abspath(__file__))
ENV = os.path.join(HARNESS, "..", "environments", "crash_recovery_env_v1")
env = tempfile.mkdtemp(prefix="zaion-rec-test-")
shutil.copytree(ENV, env, dirs_exist_ok=True)

def verify():
    r = subprocess.run(["python", os.path.join(HARNESS, "verifier.py"), "--check", "ZAION-300-REC-001", "--env", env],
                       capture_output=True, text=True, timeout=120)
    line = r.stdout.strip().splitlines()[-1] if r.stdout.strip() else "{}"
    return r.returncode, json.loads(line)

code, s = verify()
print("UNFIXED: exit=%d %s" % (code, s))
assert code == 1, "unfixed recovery env should fail"

# simulate the agent's recovery: apply journal items + mark committed
items = os.path.join(env, "data", "items.json")
journal = os.path.join(env, "journal.json")
with open(items, encoding="utf-8") as fh:
    data = json.load(fh)
with open(journal, encoding="utf-8") as fh:
    jrn = json.load(fh)
data["items"].extend(jrn["items"])
jrn["state"] = "committed"
jrn["committed_at"] = "2026-08-14T12:00:00Z"
with open(items, "w", encoding="utf-8") as fh:
    json.dump(data, fh)
with open(journal, "w", encoding="utf-8") as fh:
    json.dump(jrn, fh)

code, s = verify()
print("FIXED:   exit=%d %s" % (code, s))
assert code == 0, "recovered env should pass"
shutil.rmtree(env, ignore_errors=True)
print("RECOVERY VERIFIER E2E OK")
