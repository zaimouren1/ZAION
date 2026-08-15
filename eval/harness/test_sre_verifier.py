#!/usr/bin/env python3
"""SRE verifier E2E: unfixed fails, fixed passes."""
import json, os, shutil, subprocess, sys, tempfile
HARNESS = os.path.dirname(os.path.abspath(__file__))
SRE = os.path.join(HARNESS, "..", "environments", "sre_env_v1")
env = tempfile.mkdtemp(prefix="zaion-sre-test-")
shutil.copytree(SRE, env, dirs_exist_ok=True)

def verify():
    r = subprocess.run(["python", os.path.join(HARNESS, "verifier.py"), "--check", "ZAION-300-HERO-007", "--env", env],
                       capture_output=True, text=True, timeout=120)
    line = r.stdout.strip().splitlines()[-1] if r.stdout.strip() else "{}"
    return r.returncode, json.loads(line)

code, s = verify()
print("UNFIXED: exit=%d %s" % (code, s))
assert code == 1, "unfixed SRE env should fail"

svc = os.path.join(env, "service.py")
with open(svc, encoding="utf-8") as fh:
    src = fh.read()
src = src.replace('httpd = HTTPServer(("127.0.0.1", 8080), Handler)', 'httpd = HTTPServer(("127.0.0.1", port), Handler)')
src = src.replace("healthy = items <= 10", "healthy = items <= cfg.get('service', {}).get('max_items', 5)")
with open(svc, "w", encoding="utf-8") as fh:
    fh.write(src)

code, s = verify()
print("FIXED:   exit=%d %s" % (code, s))
assert code == 0, "fixed SRE env should pass"
shutil.rmtree(env, ignore_errors=True)
print("SRE VERIFIER E2E OK")
