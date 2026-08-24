#!/usr/bin/env python3
"""Security Metrics: 顶层安全指标量化（Security Escape Rate）。

把"有多少安全测试"量化成"穿透率"：
  Escape Rate = 失败的安全对抗测试 / 总安全对抗测试

4 个安全维度 + 测试名关键词：
  Auth Bypass  : auth, token, bearer, unauthorized, deny, reject, missing
  RBAC         : rbac, role, permission, admin, allow
  SSRF         : ssrf, redirect, url, host, dns, socket, internal
  Secret Leak  : secret, redact, key, credential, leak, expose
"""
import subprocess, sys, re, json, os, time

DIMENSIONS = {
  "auth_bypass": ["auth", "token", "bearer", "unauthorized", "deny", "reject", "missing", "forbidden"],
  "rbac": ["rbac", "role", "permission", "admin", "allow"],
  "ssrf": ["ssrf", "redirect", "url", "host", "dns", "socket", "internal", "pin"],
  "secret_leak": ["secret", "redact", "key", "credential", "leak", "expose"],
}

CRATES = ["zaion-gateway", "zaion-safety", "zaion-mcp", "zaion-secrets"]

def test_names(crate):
    r = subprocess.run(["cargo", "test", "-p", crate, "--", "--list"], capture_output=True, text=True, timeout=600, encoding="utf-8", errors="replace")
    names = re.findall(r"^(\S+)::(\S+): test$", r.stdout, re.M)
    return [n[1] for n in names]

def classify(name):
    low = name.lower()
    for dim, kws in DIMENSIONS.items():
        if any(k in low for k in kws):
            return dim
    return None

def main():
    rows = {}
    for crate in CRATES:
        names = test_names(crate)
        t0 = time.time()
        r = subprocess.run(["cargo", "test", "-p", crate, "--quiet"], capture_output=True, text=True, timeout=900, encoding="utf-8", errors="replace")
        secs = round(time.time() - t0, 1)
        failed = set(re.findall(r"test (\S+::\S+) ... FAILED", r.stdout + r.stderr))
        for name in names:
            dim = classify(name)
            if dim is None: continue
            full = crate + "::" + name
            rows.setdefault(dim, {"total": 0, "failed": 0})
            rows[dim]["total"] += 1
            if any(full.endswith(f) for f in failed):
                rows[dim]["failed"] += 1
    # 生成报告
    lines = ["# Security Metrics（顶层安全指标）", "", "| 维度 | 对抗测试数 | 穿透数 | Escape Rate | 结论 |", "|---|---|---|---|---|"]
    for dim in ["auth_bypass", "rbac", "ssrf", "secret_leak"]:
        d = rows.get(dim, {"total": 0, "failed": 0})
        total = d["total"]; failed = d["failed"]
        rate = (failed / total * 100) if total else 0
        verdict = "SAFE (0% escape)" if rate == 0 else ("LEAK (" + str(round(rate,1)) + "%)" )
        lines.append("| %s | %d | %d | %.1f%% | %s |" % (dim, total, failed, rate, verdict))
    lines.append("")
    lines.append("Security Escape Rate = 穿透数 / 对抗测试数（本应被阻断的攻击中，实际穿透的比例）。0% = 全部阻断。")
    os.makedirs("eval/results", exist_ok=True)
    open("eval/results/SECURITY_METRICS.md", "w", encoding="utf-8").write("\n".join(lines) + "\n")
    print("\n".join(lines))
    return 0

if __name__ == "__main__":
    sys.exit(main())