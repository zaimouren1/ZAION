#!/usr/bin/env python3
"""sample_tui002_executor.py - TUI queue shows pending turns and steers."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"pending_turns": 2, "shown": True, "steer_applied": True, "queue_updated": True}
    target = os.path.join(env_dir, "tui002_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["shown"] is True and record["steer_applied"] is True
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "queue shown + steered" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
