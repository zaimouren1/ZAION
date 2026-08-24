#!/usr/bin/env python3
"""Hero Eval: 4 个真实 LLM 场景（evidence_level 5）的可复现验证。

场景: 代码修复 / SRE 配置 / 崩溃恢复 / 安全验证。
需要: zaion 二进制 + ANTHROPIC_API_KEY + ANTHROPIC_BASE_URL。

用法: python hero_eval.py <zaion_bin> [--quick]
"""
import subprocess, sys, os, re, tempfile, shutil, time, json

SCENARIOS = [
  ("code-fix", "sandbox_repo_v1", "Run cargo test and fix the failing tests in src/lib.rs.", "verify_code"),
  ("sre-config", "sre_env_v1", "The service ignores its config: binds port 8080 instead of 9090 and uses threshold 10 instead of 5. Fix service.py.", "verify_sre"),
  ("crash-recovery", "crash_recovery_env_v1", "Apply the pending journal in journal.json to data/items.json and mark it committed.", "verify_recovery"),
  ("security", "security_env_v1", "Verify both receipts in receipts/ using verify_receipt.py and write verification_report.json.", "verify_security"),
]

def run(bin, args, cwd=None, env=None, timeout=900):
    r = subprocess.run([bin] + args, capture_output=True, text=True, cwd=cwd, env=env, timeout=timeout, encoding="utf-8", errors="replace")
    return r.returncode, r.stdout + r.stderr

def verify_code(work):
    r = subprocess.run(["cargo", "test", "--", "--test-threads=1"], capture_output=True, text=True, cwd=work, timeout=300, encoding="utf-8", errors="replace")
    return "6 passed" in (r.stdout + r.stderr) and "0 failed" in (r.stdout + r.stderr)

def verify_sre(work):
    src = open(os.path.join(work, "service.py"), encoding="utf-8", errors="replace").read()
    return "HTTPServer((" in src and "port" in src and "max_items" in src

def verify_recovery(work):
    items = open(os.path.join(work, "data", "items.json"), encoding="utf-8", errors="replace").read()
    journal = open(os.path.join(work, "journal.json"), encoding="utf-8", errors="replace").read()
    return "[1, 2, 3, 4, 5]" in items and "committed" in journal

def verify_security(work):
    report = os.path.join(work, "verification_report.json")
    if not os.path.exists(report): return False
    data = json.load(open(report, encoding="utf-8", errors="replace"))
    results = data.get("results", [])
    return len(results) == 2 and any(r.get("valid") for r in results) and any(not r.get("valid") for r in results)

VERIFIERS = {"verify_code": verify_code, "verify_sre": verify_sre, "verify_recovery": verify_recovery, "verify_security": verify_security}

def main():
    bin = sys.argv[1] if len(sys.argv) > 1 else "target/debug/zaion.exe"
    quick = "--quick" in sys.argv
    key = os.environ.get("ANTHROPIC_API_KEY", "")
    base = os.environ.get("ANTHROPIC_BASE_URL", "https://tokenrhythm.studio")
    if not key:
        print("set ANTHROPIC_API_KEY"); return 2
    home = tempfile.mkdtemp(prefix="zaion-hero-")
    env = dict(os.environ); env["ZAION_HOME"] = home; env["ANTHROPIC_BASE_URL"] = base
    answers = "0\n%s\n%s\ndeepseek-v4-pro-0813\n\ndefault\n" % (key, base)
    subprocess.run([bin, "onboard"], input=answers, capture_output=True, text=True, env=env, timeout=120)
    subprocess.run([bin, "config", "set", "model", "deepseek-v4-pro-0813"], capture_output=True, text=True, env=env, timeout=120)
    cfg = open(os.path.join(home, "config.toml"), encoding="utf-8", errors="replace").read()
    pid = re.search(r"default_principal_id\s*=\s*\"([^\"]+)\"", cfg).group(1)
    results = []
    for name, envdir, msg, verifier in SCENARIOS:
        if quick and name != "code-fix": continue
        work = os.path.join(tempfile.gettempdir(), "zaion-hero-" + name)
        shutil.rmtree(work, ignore_errors=True)
        shutil.copytree(os.path.join("eval/environments", envdir), work)
        tas = os.path.join(work, "TASKS.md")
        if os.path.exists(tas): os.remove(tas)
        t0 = time.time()
        code, out = run(bin, ["hero", pid, msg], cwd=work, env=env)
        secs = round(time.time() - t0, 1)
        ok = VERIFIERS[verifier](work)
        results.append((name, ok, secs))
        print("%s: %s (%ss)" % (name, "PASS" if ok else "FAIL", secs))
    n = sum(1 for _, ok, _ in results if ok)
    print("hero eval: %d/%d pass" % (n, len(results)))
    # 写报告
    lines = ["# Hero Eval（真实 LLM，evidence_level 5）", "", "| 场景 | 结果 | 耗时 |", "|---|---|---|"]
    for name, ok, secs in results:
        lines.append("| %s | %s | %ss |" % (name, "PASS" if ok else "FAIL", secs))
    lines.append(""); lines.append("**%d/%d pass**" % (n, len(results)))
    os.makedirs("eval/results", exist_ok=True)
    open("eval/results/HERO_EVAL_REPORT.md", "w", encoding="utf-8").write("\n".join(lines) + "\n")
    return 0 if n == len(results) else 1

if __name__ == "__main__":
    sys.exit(main())