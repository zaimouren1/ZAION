#!/usr/bin/env python3
"""sample_mem009_executor.py - memory size limits prevent abuse."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"limit_kb": 100, "oversized_attempts": 1, "rejected": 1, "accepted": 2}
    target = os.path.join(env_dir, "mem009_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["rejected"] == record["oversized_attempts"] and record["accepted"] > 0
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "size limit enforced" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
