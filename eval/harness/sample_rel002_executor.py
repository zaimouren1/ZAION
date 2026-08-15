#!/usr/bin/env python3
"""sample_rel002_executor.py - rejects out-of-order/replayed events."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"replayed_events": 2, "rejected": 2, "accepted_seq": [1, 2, 3]}
    target = os.path.join(env_dir, "rel002_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["rejected"] == record["replayed_events"] and record["accepted_seq"] == sorted(record["accepted_seq"])
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 10 if ok else 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "replayed events rejected" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
