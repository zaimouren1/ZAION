#!/usr/bin/env python3
"""sample_sre_executor.py - fixes the SRE service's two config defects.

  BUG-S1: bind the configured service.port (not hardcoded 8080)
  BUG-S2: /status threshold from config service.max_items (not hardcoded 10)
"""
import json, os, sys

FIXES = [
    ("httpd = HTTPServer((\"127.0.0.1\", 8080), Handler)",
     "httpd = HTTPServer((\"127.0.0.1\", port), Handler)"),
    ("healthy = items <= 10",
     "healthy = items <= cfg.get(\"service\", {}).get(\"max_items\", 5)"),
]


def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    svc_path = os.path.join(env_dir, "service.py")
    if not os.path.exists(svc_path):
        print(json.dumps({"task_id": task["id"], "success": 0, "rework": 0, "recovery": 0,
                          "trust": 0, "cost_latency": 0, "evidence_path": svc_path,
                          "notes": "service.py missing"}))
        return 1
    with open(svc_path, encoding="utf-8") as fh:
        src = fh.read()
    applied = 0
    for old, new in FIXES:
        if old in src:
            src = src.replace(old, new, 1)
            applied += 1
    with open(svc_path, "w", encoding="utf-8") as fh:
        fh.write(src)
    ok = applied == 2
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10 if ok else 0, "cost_latency": 0,
                      "evidence_path": svc_path,
                      "notes": "SRE defects fixed" if ok else "only %d/2 fixed" % applied}))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
