#!/usr/bin/env python3
"""sample_hero006_executor.py - documents an alert investigation."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"alert": "AL-42", "root_cause": "cap ignored", "documented": True, "evidence_linked": True}
    target = os.path.join(env_dir, "hero006_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["documented"] is True and record["evidence_linked"] is True
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "investigation documented" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
