#!/usr/bin/env python3
"""sample_executor.py - demonstrates the executor contract for benchmark runner.

The REAL executor is the agent under evaluation (same model/budget/timeout).
This sample runs the task verifier and reports dimensions honestly:
  - success: 10 if verifier passes (tests green), 0 otherwise
  - rework: 10 if no manual fixes were needed (unknown here -> 0)
  - recovery: 10 if no faults injected (unknown -> 0)
  - trust: 10 if verifier is independent (it is -> 10)
  - cost_latency: 10 if run was cheap (unknown -> 0)
Used with: python runner.py --run TASK_ID --executor "python eval/harness/sample_executor.py" --env DIR
"""
import json, os, subprocess, sys

def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    verifier = os.path.join(os.path.dirname(os.path.abspath(__file__)), "verifier.py")
    proc = subprocess.run(["python", verifier, "--check", task["id"], "--env", env_dir],
                          capture_output=True, text=True, timeout=300)
    passed = proc.returncode == 0
    result = {
        "task_id": task["id"],
        "success": 10 if passed else 0,
        "rework": 0,
        "recovery": 0,
        "trust": 10,  # independent verifier
        "cost_latency": 0,
        "evidence_path": None,
        "notes": "sample executor: verifier %s" % ("passed" if passed else "failed"),
    }
    print(json.dumps(result))

if __name__ == "__main__":
    main()
