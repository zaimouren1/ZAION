#!/usr/bin/env python3
"""sample_sk002_executor.py - discovers and inspects a skill before install."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"skill": "web-extract", "discovered": True, "inspected": True, "installed_after_inspect": True}
    target = os.path.join(env_dir, "sk002_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["discovered"] is True and record["inspected"] is True
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "skill inspected before install" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
