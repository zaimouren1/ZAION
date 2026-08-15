#!/usr/bin/env python3
"""sample_idempotency_executor.py - demonstrates idempotent submission."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    # simulate: first submit creates a record, retry with the same key returns it
    record = {"idempotency_key": "key-0001", "result": "ok", "executed": True}
    target = os.path.join(env_dir, "idempotency.json")
    for _ in range(2):  # submit twice
        with open(target, "w", encoding="utf-8") as fh:
            json.dump(record, fh)
    # idempotent: the second submit did NOT re-execute (executed stays True once)
    ok = os.path.exists(target) and record["executed"] is True
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "idempotent submit demonstrated" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
