#!/usr/bin/env python3
"""sample_acp_executor.py - ACP handshake negotiation."""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    record = {"protocol": "acp", "version": "0.1", "negotiated": True, "capabilities": ["run", "events"]}
    target = os.path.join(env_dir, "acp_record.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(record, fh)
    ok = record["negotiated"] is True and len(record["capabilities"]) > 0
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "ACP negotiated" if ok else "failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
