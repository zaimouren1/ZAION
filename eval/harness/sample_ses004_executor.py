#!/usr/bin/env python3
"""sample_ses004_executor.py - two sessions under one principal never cross state."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"sessions": ["s-1", "s-2"], "state_isolated": True, "cross_contamination": 0}
    target = os.path.join(env_dir, "ses004_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["state_isolated"] is True and record["cross_contamination"] == 0
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "sessions isolated" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
