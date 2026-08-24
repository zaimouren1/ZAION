#!/usr/bin/env python3
"""Cross-System Eval v2: Capability Correctness 分层诊断。

不只检查"各命令 exit 0"，而是验证每层语义正确性 + 定位哪层坏。

分层: L1 fact 溯源 -> L2 context principal -> L3 context skill ->
      L4 ledger 事件 -> L5 sync 导出

端到端结论: 所有层 PASS = Capability Correct; 某层 FAIL = 定位该层。
"""
import subprocess, sys, os, re, tempfile

def run(bin, args, env, timeout=120):
    r = subprocess.run([bin] + args, capture_output=True, text=True, env=env, timeout=timeout, encoding="utf-8", errors="replace")
    return r.returncode, r.stdout + r.stderr

def main():
    bin = sys.argv[1] if len(sys.argv) > 1 else "target/debug/zaion.exe"
    home = tempfile.mkdtemp(prefix="zaion-xs-")
    env = dict(os.environ); env["ZAION_HOME"] = home
    answers = "0\nsk-tr-placeholder\nhttps://tokenrhythm.studio\ndeepseek-v4-pro-0813\n\ndefault\n"
    subprocess.run([bin, "onboard"], input=answers, capture_output=True, text=True, env=env, timeout=120)
    cfg = open(os.path.join(home, "config.toml"), encoding="utf-8", errors="replace").read()
    pid = re.search(r"default_principal_id\s*=\s*\"([^\"]+)\"", cfg).group(1)

    layers = []

    # L1: fact 溯源（memory atom -> provenance）
    c, o = run(bin, ["memory", "add-fact", pid, "job-change", "user changed job to acme", "--user-provided"], env)
    c2, o2 = run(bin, ["memory", "graph", pid], env)
    ok = c == 0 and "user-provided" in o2 and "[fact]" in o2
    layers.append(("L1 fact-provenance", ok, "memory atom traceable to source"))

    # L2: context principal 层（身份进上下文）
    c3, o3 = run(bin, ["context", "build", pid], env)
    ok = "principal" in o3 and "small-octopus" in o3
    layers.append(("L2 context-principal", ok, "identity layer assembled into context"))

    # L3: context skill 层（skill learn -> query 匹配进上下文）
    run(bin, ["skill", "learn", "terse answers preferred"], env)
    c4, o4 = run(bin, ["context", "build", pid, "--query", "terse"], env)
    ok = "terse" in o4.lower() and "skill" in o4.lower()
    layers.append(("L3 context-skill", ok, "learned skill retrieved into context by query"))

    # L4: ledger 事件（签名 + 不可抵赖）
    c5, o5 = run(bin, ["events", pid, "--json"], env)
    ok = c5 == 0 and "process.created" in o5 and "public_key" in o5
    layers.append(("L4 ledger-event", ok, "signed event with public_key"))

    # L5: sync 导出（事件进 bundle）
    out = os.path.join(home, "bundle.zaionsync")
    c6, o6 = run(bin, ["sync", "export", pid, "--out", out], env)
    ok = c6 == 0 and os.path.exists(out) and os.path.getsize(out) > 0
    layers.append(("L5 sync-export", ok, "events exported to bundle"))

    # 诊断报告
    print("Cross-System Capability Correctness:")
    failed = []
    for name, ok, desc in layers:
        mark = "PASS" if ok else "FAIL"
        print("  [" + mark + "] " + name + ": " + desc)
        if not ok: failed.append(name)
    n = sum(1 for _, ok, _ in layers if ok)
    if failed:
        print("END-TO-END: FAILED at " + ", ".join(failed))
        print("diagnosis: healthy layers upstream, capability broken at the listed layer(s)")
    else:
        print("END-TO-END: Capability Correct (all layers healthy)")
    print("total %d | pass %d | fail %d" % (len(layers), n, len(layers) - n))
    return 0 if not failed else 1

if __name__ == "__main__":
    sys.exit(main())