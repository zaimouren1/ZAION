#!/usr/bin/env python3
"""sample_mem002_executor.py - invalidates a memory atom after source change."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"atom": "a-1", "source_changed": True, "invalidated": True, "new_atom_written": True}
    target = os.path.join(env_dir, "mem002_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["source_changed"] is True and record["invalidated"] is True and record["new_atom_written"] is True
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "atom invalidated" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
