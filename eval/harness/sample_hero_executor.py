#!/usr/bin/env python3
"""sample_hero_executor.py - fixes the sandbox repo's three deliberate defects.

Applies the designer-known fixes to src/lib.rs (the answer key is NOT given
to real agents, but this sample demonstrates a correctly-solving agent):
  BUG-1: process_batch honors the cap
  BUG-2: validate_token requires the "zx" prefix
  BUG-3: format_item renders 1-based labels
"""
import json, os, sys

FIXES = [
    ("let _ = cap; // BUG-1: cap is ignored; should limit items.len()\n    items.iter().sum()",
     "items.iter().take(cap).sum()"),
    ("token.starts_with(\"zk\")", "token.starts_with(\"zx\")"),
    ("format!(\"item {}: {}\", index, value)", "format!(\"item {}: {}\", index + 1, value)"),
]


def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    lib_path = os.path.join(env_dir, "src", "lib.rs")
    if not os.path.exists(lib_path):
        print(json.dumps({"task_id": task["id"], "success": 0, "rework": 0, "recovery": 0,
                          "trust": 0, "cost_latency": 0, "evidence_path": lib_path,
                          "notes": "lib.rs missing"}))
        return 1
    with open(lib_path, encoding="utf-8") as fh:
        src = fh.read()
    applied = 0
    for old, new in FIXES:
        if old in src:
            src = src.replace(old, new, 1)
            applied += 1
    with open(lib_path, "w", encoding="utf-8") as fh:
        fh.write(src)
    ok = applied == 3
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10 if ok else 0, "cost_latency": 0,
                      "evidence_path": lib_path,
                      "notes": "3 defects fixed" if ok else "only %d/3 fixed" % applied}))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
