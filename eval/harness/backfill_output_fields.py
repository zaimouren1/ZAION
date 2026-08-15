#!/usr/bin/env python3
"""backfill_output_fields.py - derive task output artifacts from sample executors
and write them into the manifest's output field (structured acceptance).

Reads run_suite.ps1 runs (task_id, executor), scans each sample executor for
the artifact filename it writes (os.path.join(env_dir, "...")), and updates
the manifest tasks accordingly.
"""
import json, os, re, subprocess

ROOT = "D:/zaion-rust"
SUITE = os.path.join(ROOT, "eval", "harness", "run_suite.ps1")
MANIFEST = os.path.join(ROOT, "eval", "benchmarks", "zaion_300_v1.json")

with open(SUITE, encoding="utf-8") as fh:
    suite = fh.read()
# find executor variable assignments and runs
vars_map = dict(re.findall(r"\$(\w+) = \"(python D:/zaion-rust/eval/harness/\w+\.py)\"", suite))
runs = re.findall(r"@\(\"(ZAION-300-[\w-]+)\", \$(\w+)\)", suite)
print("executors found:", len(vars_map), "runs:", len(runs))

# task_id -> executor path
task_exec = {}
for tid, var in runs:
    if var in vars_map:
        task_exec[tid] = vars_map[var].replace("python D:/zaion-rust/eval/harness/", "")

# scan executors for the artifact filename
def artifact_of(exe):
    path = os.path.join(ROOT, "eval", "harness", exe)
    if not os.path.exists(path):
        return None
    with open(path, encoding="utf-8") as fh:
        src = fh.read()
    m = re.search(r'os\.path\.join\(env_dir, \"([\w.]+)\"\)', src)
    return m.group(1) if m else None

with open(MANIFEST, encoding="utf-8") as fh:
    manifest = json.load(fh)
updated = 0
for task in manifest["tasks"]:
    tid = task.get("id", "")
    if tid in task_exec and not task.get("output"):
        art = artifact_of(task_exec[tid])
        if art:
            task["output"] = {"path": art, "format": "see verifier (JSON artifact checked by the benchmark verifier)"}
            updated += 1
with open(MANIFEST, "w", encoding="utf-8") as fh:
    json.dump(manifest, fh, ensure_ascii=False, indent=2)
print("tasks updated with output:", updated)