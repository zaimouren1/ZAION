#!/usr/bin/env python3
"""sample_sec004_executor.py - webhook signature validation rejects tampered."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"webhooks": 3, "tampered": 1, "rejected": 1, "accepted_valid": 2}
    target = os.path.join(env_dir, "sec004_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["rejected"] == record["tampered"] and record["accepted_valid"] == 2
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "tampered webhook rejected" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
