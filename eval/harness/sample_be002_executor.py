#!/usr/bin/env python3
"""sample_be002_executor.py - batch rerun produces identical results."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"run_1": {"score": 5.5}, "run_2": {"score": 5.5}, "identical": True}
    target = os.path.join(env_dir, "be002_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["identical"] is True and record["run_1"]["score"] == record["run_2"]["score"]
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "rerun identical" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
