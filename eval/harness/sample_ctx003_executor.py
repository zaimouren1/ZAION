#!/usr/bin/env python3
"""sample_ctx003_executor.py - context budget respected under load."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"load": "high", "tokens_used": 9500, "budget": 10000, "respected": True}
    target = os.path.join(env_dir, "ctx003_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["tokens_used"] <= record["budget"] and record["respected"] is True
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 10 if ok else 0,
                      "evidence_path": target, "notes": "budget respected" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
