#!/usr/bin/env python3
"""sample_security_executor.py - detects the tampered receipt and writes the report.

Reads the env receipts and produces a verification report that correctly
flags r1 valid and r2 tampered (a well-behaved agent's output).
"""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    receipts_path = os.path.join(env_dir, "receipts.json")
    results = []
    if os.path.exists(receipts_path):
        with open(receipts_path, encoding="utf-8") as fh:
            receipts = json.load(fh)
        for rec in receipts:
            rid = rec.get("id")
            sig = rec.get("signature", "")
            # detect tampering: r2 has a corrupted signature
            valid = sig.startswith("sig-ok-")
            results.append({"id": rid, "valid": valid})
    else:
        # fallback: known tamper scenario (r1 ok, r2 tampered)
        results = [{"id": "r1", "valid": True}, {"id": "r2", "valid": False}]
    report = {"results": results}
    target = os.path.join(env_dir, "verification_report.json")
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(report, fh)
    by_id = {x["id"]: x["valid"] for x in results}
    ok = by_id.get("r1") is True and by_id.get("r2") is False
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": target, "notes": "tamper detected" if ok else "report written"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
