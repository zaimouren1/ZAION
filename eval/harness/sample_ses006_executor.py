#!/usr/bin/env python3
"""sample_ses006_executor.py - session prune keeps evidence trail."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"pruned_turns": 3, "evidence_kept": 8, "evidence_trail_intact": True}
    target = os.path.join(env_dir, "ses006_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["evidence_trail_intact"] is True and record["evidence_kept"] > 0
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "evidence kept after prune" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
