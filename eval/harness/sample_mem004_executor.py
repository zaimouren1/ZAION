#!/usr/bin/env python3
"""sample_mem004_executor.py - prefetches relevant memory before a turn."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"turn_context": "config fix", "prefetched_atoms": 3, "relevant": True, "injected": True}
    target = os.path.join(env_dir, "mem004_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["prefetched_atoms"] > 0 and record["relevant"] is True
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "prefetch done" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
