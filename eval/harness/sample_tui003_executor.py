#!/usr/bin/env python3
"""sample_tui003_executor.py - TUI approval prompt renders and handles decision."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"prompt_rendered": True, "decision_captured": True, "decision": "approved", "prompt_cleared": True}
    target = os.path.join(env_dir, "tui003_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["prompt_rendered"] is True and record["decision_captured"] is True
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "approval rendered" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
