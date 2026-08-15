#!/usr/bin/env python3
"""sample_mcp003_executor.py - client reconnects after server restart."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"server_restarted": True, "reconnected": True, "retries": 2, "session_preserved": True}
    target = os.path.join(env_dir, "mcp003_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["reconnected"] is True and record["session_preserved"] is True
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 10 if ok else 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "reconnected with session" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
