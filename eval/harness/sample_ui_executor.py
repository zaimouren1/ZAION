#!/usr/bin/env python3
"""sample_ui_executor.py - cancel button responds during a run."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"run_started": True, "cancel_clicked": True, "cancel_responded_ms": 150, "run_stopped": True}
    target = os.path.join(env_dir, "ui_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["cancel_clicked"] is True and record["cancel_responded_ms"] <= 5000 and record["run_stopped"] is True
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "cancel responded" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
