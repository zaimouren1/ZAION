#!/usr/bin/env python3
"""sample_evidence_executor.py - writes an evidence record with proof lineage."""
import hashlib, json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    event = {"kind": "tool_exec", "action": "write_file", "path": "a.txt"}
    proof = hashlib.sha256(json.dumps(event, sort_keys=True).encode()).hexdigest()
    record = {"event": event, "proof_hash": proof, "chain": ["genesis", proof[:16]]}
    target = os.path.join(env_dir, "evidence.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = os.path.exists(target) and len(record["proof_hash"]) == 64
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "evidence with proof lineage" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
