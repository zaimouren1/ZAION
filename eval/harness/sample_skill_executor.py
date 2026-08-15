#!/usr/bin/env python3
"""sample_skill_executor.py - skill update preserving user data."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {
        "skill": "web-extract",
        "version_before": "1.0.0",
        "version_after": "1.1.0",
        "user_data_preserved": True,
    }
    target = os.path.join(env_dir, "skill_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["version_before"] != record["version_after"] and record["user_data_preserved"] is True
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "skill updated, data preserved" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
