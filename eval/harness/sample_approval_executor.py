#!/usr/bin/env python3
"""sample_approval_executor.py - demonstrates the approval flow (denied)."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {
        "proposed_action": "rm -rf /data/cache",
        "approval_requested": True,
        "decision": "denied",
        "executed": False,
    }
    target = os.path.join(env_dir, "approval_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = os.path.exists(target) and record["executed"] is False and record["decision"] == "denied"
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "approval denied, no execution" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
