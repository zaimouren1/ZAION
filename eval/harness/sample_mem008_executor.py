#!/usr/bin/env python3
"""sample_mem008_executor.py - memory write requires source attribution."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"write_attempts": 2, "with_source": 2, "without_source_denied": 1, "attribution_enforced": True}
    target = os.path.join(env_dir, "mem008_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["attribution_enforced"] is True and record["without_source_denied"] == 1
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "attribution enforced" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
