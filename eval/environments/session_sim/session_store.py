#!/usr/bin/env python3
"""session_sim - simulated session store for SES-* benchmark tasks.

A JSON store of sessions with lineage chains. The agent can read/write
it like a real session runtime would present state. Designer-only
TASKS.md is stripped by the runner."""
import json, os

STORE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "sessions.json")

def load():
    with open(STORE, encoding="utf-8") as fh:
        return json.load(fh)

def save(data):
    with open(STORE, "w", encoding="utf-8") as fh:
        json.dump(data, fh, ensure_ascii=False, indent=2)

def create(store, sid, parent=None, lines_n=0):
    lineage = list(store.get("lineage_of", {}).get(parent, [])) if parent else []
    lineage = lineage + [sid]
    store["sessions"][sid] = {"parent": parent, "lines": lines_n, "created": True}
    store["lineage_of"][sid] = lineage
    return lineage

if __name__ == "__main__":
    store = {"sessions": {"s-root": {"parent": None, "lines": 3, "created": True}},
             "lineage_of": {"s-root": ["s-root"]}}
    save(store)
    print("session_sim store initialized:", STORE)