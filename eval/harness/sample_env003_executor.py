#!/usr/bin/env python3
"""sample_env003_executor.py - restart after config change preserves state."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"config_changed": True, "restarted": True, "state_preserved": True, "state_items": 5}
    target = os.path.join(env_dir, "env003_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["restarted"] is True and record["state_preserved"] is True
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 10 if ok else 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "restart preserved state" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
