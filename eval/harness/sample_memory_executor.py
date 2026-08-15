#!/usr/bin/env python3
"""sample_memory_executor.py - demonstrates a memory-write task.

Writes a memory atom (JSON with text + source binding) into the env, as a
well-behaved agent would. The verifier checks source attribution + content.
"""
import json, os, sys


def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    atom = {
        "text": "The sandbox service honors its configured port after the fix.",
        "source": "sre_env_v1/logs/incident.log",
        "created_at": "2026-08-14T00:00:00Z",
    }
    target = os.path.join(env_dir, "memory_atoms.jsonl")
    with open(target, "w", encoding="utf-8") as fh:
        fh.write(json.dumps(atom) + "\n")
    ok = os.path.exists(target)
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "memory atom written" if ok else "write failed"}))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
