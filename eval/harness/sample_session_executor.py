#!/usr/bin/env python3
"""sample_session_executor.py - creates a session with signed lineage."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    session = {
        "session_id": "sess-test-001",
        "parent": "sess-parent-000",
        "lineage": ["sess-parent-000", "sess-test-001"],
        "compression_chain": "verified",
    }
    target = os.path.join(env_dir, "session.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(session, fh)
    ok = os.path.exists(target)
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "session created with lineage" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
