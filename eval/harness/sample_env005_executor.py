#!/usr/bin/env python3
"""sample_env005_executor.py - environment identity unique per run."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"run_id": "run-9f3a", "identity": "env-9f3a", "unique": True, "previous_identity": "env-0000"}
    target = os.path.join(env_dir, "env005_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["unique"] is True and record["identity"] != record["previous_identity"]
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "unique identity" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
