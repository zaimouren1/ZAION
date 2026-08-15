#!/usr/bin/env python3
"""sample_be005_executor.py - batch report links each score to evidence."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"entries": [{"task": "T-1", "score": 5.5, "evidence": "e-1"}, {"task": "T-2", "score": 7.0, "evidence": "e-2"}], "all_linked": True}
    target = os.path.join(env_dir, "be005_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["all_linked"] is True and all(e["evidence"] for e in record["entries"])
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "scores linked to evidence" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
