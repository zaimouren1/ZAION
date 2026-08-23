#!/usr/bin/env python3
"""Cross-System Eval: 验证语义链各环节的数据流转正确性（不依赖真实 LLM）。

语义链: memory → context → runtime(ledger) → sync
验证点:
  1. memory 写事实 → 可溯源
  2. context 构建 → 引用 memory 层
  3. ledger 记录签名事件（不可抵赖）
  4. sync 导出 → 包含事件

用法: python cross_system_eval.py <zaion_bin> <pid>
"""
import subprocess, sys, os, json, tempfile

def run(bin, args, env):
    r = subprocess.run([bin] + args, capture_output=True, text=True, env=env, timeout=120, encoding="utf-8", errors="replace")
    return r.returncode, r.stdout + r.stderr

def main():
    bin = sys.argv[1] if len(sys.argv) > 1 else "target/debug/zaion.exe"
    pid = sys.argv[2] if len(sys.argv) > 2 else None
    home = tempfile.mkdtemp(prefix="zaion-xs-")
    env = dict(os.environ)
    env["ZAION_HOME"] = home
    # onboard（快速创建身份）
    answers = "0\nsk-tr-placeholder\nhttps://tokenrhythm.studio\ndeepseek-v4-pro-0813\n\ndefault\n"
    r = subprocess.run([bin, "onboard"], input=answers, capture_output=True, text=True, env=env, timeout=120, encoding="utf-8", errors="replace")
    if pid is None:
        # 从 config 读 principal
        import re
        m = re.search(r"default_principal_id\s*=\s*\"([^\"]+)\"", open(os.path.join(home, "config.toml"), encoding="utf-8", errors="replace").read())
        pid = m.group(1) if m else ""
    checks = []
    # 1. memory 写事实
    c1 = run(bin, ["memory", "add-fact", pid, "xs-check", "cross-system fact", "--user-provided"], env)
    checks.append(("memory.write", c1[0] == 0, "memory add-fact"))
    # 2. memory 溯源（graph）
    c2 = run(bin, ["memory", "graph", pid], env)
    checks.append(("memory.provenance", "user-provided" in c2[1] and "[fact]" in c2[1], "memory graph traces the fact to its source"))
    # 3. context 构建
    c3 = run(bin, ["context", "build", pid], env)
    checks.append(("context.build", c3[0] == 0, "context build"))
    # 4. ledger 事件
    c4 = run(bin, ["events", pid, "--json"], env)
    checks.append(("ledger.events", c4[0] == 0 and "process.created" in c4[1] and "public_key" in c4[1], "signed process.created event present"))
    # 5. sync 导出
    out = os.path.join(home, "bundle.zaionsync")
    c5 = run(bin, ["sync", "export", pid, "--out", out], env)
    checks.append(("sync.export", c5[0] == 0 and os.path.exists(out), "sync export bundle"))
    # 汇总
    n_pass = sum(1 for _, ok, _ in checks if ok)
    print("Cross-System Eval:")
    for name, ok, desc in checks:
        mark = "PASS" if ok else "FAIL"
        print("  [" + mark + "] " + name + ": " + desc)
    print(f"total {len(checks)} | pass {n_pass} | fail {len(checks)-n_pass}")
    return 0 if n_pass == len(checks) else 1

if __name__ == "__main__":
    sys.exit(main())