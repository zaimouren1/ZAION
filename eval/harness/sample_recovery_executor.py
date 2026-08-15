#!/usr/bin/env python3
"""sample_recovery_executor.py - replays the pending journal into data.

Reads the pending journal and applies its items to data/items.json, then
marks the journal committed (a well-behaved agent's crash recovery).
"""
import json, os, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    data_path = os.path.join(env_dir, "data", "items.json")
    journal_path = os.path.join(env_dir, "journal.json")
    os.makedirs(os.path.dirname(data_path), exist_ok=True)
    items = []
    journal = {"items": [], "state": "pending"}
    if os.path.exists(data_path):
        with open(data_path, encoding="utf-8") as fh:
            items = json.load(fh).get("items", [])
    if os.path.exists(journal_path):
        with open(journal_path, encoding="utf-8") as fh:
            journal = json.load(fh)
    for item in journal.get("items", []):
        if item not in items:
            items.append(item)
    with open(data_path, "w", encoding="utf-8") as fh:
        json.dump({"items": items}, fh)
    journal["state"] = "committed"
    with open(journal_path, "w", encoding="utf-8") as fh:
        json.dump(journal, fh)
    ok = all(i in items for i in journal.get("items", []))
    print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                      "recovery": 10 if ok else 0, "trust": 10, "cost_latency": 0,
                      "evidence_path": data_path, "notes": "journal replayed + committed" if ok else "recovery failed"}))
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
