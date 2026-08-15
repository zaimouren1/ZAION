#!/usr/bin/env python3
"""sample_file_executor.py - demonstrates a file-operation task.

Writes a marker file with the expected content (what a well-behaved agent
would produce for the read-before-edit file task). The verifier then checks
the file exists with correct content.
"""
import json, os, sys


def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    target = os.path.join(env_dir, "notes.txt")
    with open(target, "w", encoding="utf-8") as fh:
        fh.write("zaion file-op task: content written with read-before-edit\n")
    ok = os.path.exists(target)
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target,
                      "notes": "file written" if ok else "file write failed"}))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
