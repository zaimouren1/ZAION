#!/usr/bin/env python3
"""sample_mcp002_executor.py - tool list scoped by policy."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"policy": "readonly", "tools_returned": ["read_file", "list"], "blocked_tools": ["write_file", "delete"]}
    target = os.path.join(env_dir, "mcp002_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = "write_file" not in record["tools_returned"] and len(record["blocked_tools"]) > 0
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "tool list scoped" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
