#!/usr/bin/env python3
"""benchmark runner skeleton for Zaion 300-task eval.

Pipeline: manifest -> setup -> execute (pluggable executor) -> collect -> score -> report.

Usage:
  runner.py --list                        List tasks by category/type.
  runner.py --setup TASK_ID [--env DIR]   Prepare environment (copy sandbox, apply fault injection).
  runner.py --run TASK_ID --executor CMD [--env DIR] [--dry-run] [--budget TOKENS]
      Invoke executor against prepared env; capture result JSON.
  runner.py --score RESULT_JSON           Compute risk-adjusted score.
  runner.py --report RESULT_DIR           Aggregate scores into a report.

Executor contract: CMD receives env dir (arg 1) and task JSON (arg 2, file path);
writes one result JSON line to stdout:
  {"task_id": "...", "success": 0-10, "rework": 0-10, "recovery": 0-10,
   "trust": 0-10, "cost_latency": 0-10, "evidence_path": "...", "notes": "..."}
Dimensions 0-10; higher is better for all (cost_latency 10 = cheap/fast).
"""
import argparse, json, os, shlex, shutil, subprocess, sys

WEIGHTS = {"task_success": 40, "no_human_rework": 20, "recovery": 15,
           "trust_verification": 15, "cost_latency": 10}
DIM_MAP = {"success": "task_success", "rework": "no_human_rework",
           "recovery": "recovery", "trust": "trust_verification",
           "cost_latency": "cost_latency"}

MANIFEST = os.path.join(os.path.dirname(__file__), "..", "benchmarks", "zaion_300_v1.json")
ENV_ROOT = os.path.join(os.path.dirname(__file__), "..", "environments")


def load_manifest():
    with open(MANIFEST, encoding="utf-8") as fh:
        return json.load(fh)


def find_task(m, task_id):
    for t in m.get("tasks", []):
        if t["id"] == task_id:
            return t
    return None


def cmd_list(m, args):
    tasks = m.get("tasks", [])
    by_cat = {}
    for t in tasks:
        by_cat.setdefault(t["category"], []).append(t)
    print("%-24s %-6s %-12s %s" % ("TASK", "SLOTS", "TYPE", "TITLE"))
    for cat in sorted(by_cat):
        for t in sorted(by_cat[cat], key=lambda x: x["id"]):
            print("%-24s %-6s %-12s %s" % (t["id"], t.get("slots", 1), t.get("task_type", "?"), t["title"][:60]))
    print("total: %d tasks / %d target slots" % (len(tasks), m.get("target_task_slots", 300)))


def prepare_env(task, env_dir):
    """Copy the task template into env_dir (fresh copy per task)."""
    tid = task.get("id", "")
    if "SEC-006" in tid:
        template_name = "security_env_v1"
    elif "REC-001" in tid or "REC-002" in tid:
        template_name = "crash_recovery_env_v1"
    elif tid.startswith("ZAION-300-CH"):
        template_name = "channel_sim"
    elif tid.startswith("ZAION-300-SES"):
        template_name = "session_sim"
    elif "HERO-007" in tid or "HERO-008" in tid or "ENV-003" in tid:
        template_name = "sre_env_v1"
    else:
        template_name = "sandbox_repo_v1"
    template = os.path.join(ENV_ROOT, template_name)
    if os.path.isdir(template) and not os.path.exists(os.path.join(env_dir, "Cargo.toml")):
        shutil.copytree(template, env_dir, dirs_exist_ok=True)
    # strip designer-only TASKS.md so agents cannot read the answer key
    tasks_md = os.path.join(env_dir, "TASKS.md")
    if os.path.exists(tasks_md):
        os.remove(tasks_md)
    return env_dir


def cmd_setup(m, args):
    task = find_task(m, args.setup)
    if not task:
        print("task not found: %s" % args.setup, file=sys.stderr)
        return 1
    env_dir = args.env or os.path.join(os.environ.get("TEMP", "/tmp"), "zaion-eval", task["id"])
    os.makedirs(env_dir, exist_ok=True)
    prepare_env(task, env_dir)
    print(json.dumps({"task_id": task["id"], "env": env_dir, "prepared": True}))
    return 0


def cmd_run(m, args):
    task = find_task(m, args.run)
    if not task:
        print("task not found: %s" % args.run, file=sys.stderr)
        return 1
    env_dir = args.env or os.path.join(os.environ.get("TEMP", "/tmp"), "zaion-eval", task["id"])
    os.makedirs(env_dir, exist_ok=True)
    prepare_env(task, env_dir)

    task_file = os.path.join(env_dir, "task.json")
    with open(task_file, "w", encoding="utf-8") as fh:
        json.dump(task, fh, ensure_ascii=False)

    if args.dry_run or not args.executor:
        result = {"task_id": task["id"], "success": 0, "rework": 0, "recovery": 0,
                  "trust": 0, "cost_latency": 0, "evidence_path": None,
                  "notes": "dry-run: executor not invoked"}
    else:
        exec_cmd = shlex.split(args.executor) + [env_dir, task_file]
        result = None
        try:
            proc = subprocess.run(exec_cmd, capture_output=True, text=True, timeout=args.timeout)
            if proc.returncode != 0:
                print("executor failed (%d); verifier arbitrates" % proc.returncode, file=sys.stderr)
                result = {"task_id": task["id"], "success": 0, "rework": 0, "recovery": 0,
                          "trust": 0, "cost_latency": 0, "notes": "executor failed; verifier arbitrates"}
            else:
                try:
                    result = json.loads(proc.stdout.strip().splitlines()[-1])
                except Exception as e:
                    print("invalid executor result: %s" % e, file=sys.stderr)
                    result = {"task_id": task["id"], "success": 0, "rework": 0, "recovery": 0,
                              "trust": 0, "cost_latency": 0, "notes": "invalid result; verifier arbitrates"}
        except subprocess.TimeoutExpired:
            print("executor timed out; verifier arbitrates", file=sys.stderr)
            result = {"task_id": task["id"], "success": 0, "rework": 0, "recovery": 0,
                      "trust": 0, "cost_latency": 0, "notes": "executor timed out; verifier arbitrates"}

    result["budget"] = args.budget
    out = os.path.join(env_dir, "result.json")
    with open(out, "w", encoding="utf-8") as fh:
        json.dump(result, fh, ensure_ascii=False, indent=2)
    print(json.dumps(result, ensure_ascii=False))
    return 0


def cmd_score(m, args):
    with open(args.score, encoding="utf-8") as fh:
        r = json.load(fh)
    total = 0.0
    detail = {}
    long_to_short = {v: k for k, v in DIM_MAP.items()}
    for wkey, weight in WEIGHTS.items():
        short = long_to_short.get(wkey, wkey)
        dim = r.get(short, 0)
        total += dim * weight
        detail[wkey] = dim
    risk = total / 100.0  # weighted average of 0-10 dims -> 0-10
    print(json.dumps({"task_id": r.get("task_id"), "risk_adjusted_score": round(risk, 2),
                      "dimensions": detail}, ensure_ascii=False))
    return 0


def cmd_report(m, args):
    files = []
    for root, _dirs, names in os.walk(args.report):
        for n in names:
            if n.endswith(".json"):
                files.append(os.path.join(root, n))
    scores = []
    for fp in files:
        with open(fp, encoding="utf-8") as fh:
            r = json.load(fh)
        if "success" in r:
            long_to_short = {v: k for k, v in DIM_MAP.items()}
            total = sum(r.get(long_to_short.get(k, k), 0) * w for k, w in WEIGHTS.items()) / 100.0
            scores.append({"task_id": r.get("task_id"), "score": round(total, 2)})
    avg = sum(s["score"] for s in scores) / len(scores) if scores else 0
    print(json.dumps({"tasks_scored": len(scores), "average_risk_adjusted": round(avg, 2),
                      "detail": scores}, ensure_ascii=False))
    return 0


def main():
    p = argparse.ArgumentParser(prog="benchmark_runner")
    p.add_argument("--list", action="store_true")
    p.add_argument("--setup", metavar="TASK_ID")
    p.add_argument("--run", metavar="TASK_ID")
    p.add_argument("--executor", metavar="CMD", default="")
    p.add_argument("--env", metavar="DIR", default="")
    p.add_argument("--dry-run", action="store_true")
    p.add_argument("--budget", type=int, default=20000)
    p.add_argument("--timeout", type=int, default=1800)
    p.add_argument("--score", metavar="RESULT_JSON")
    p.add_argument("--report", metavar="RESULT_DIR")
    args = p.parse_args()

    m = load_manifest()
    if args.list:
        return cmd_list(m, args)
    if args.setup:
        return cmd_setup(m, args)
    if args.run:
        return cmd_run(m, args)
    if args.score:
        return cmd_score(m, args)
    if args.report:
        return cmd_report(m, args)
    p.print_help()
    return 1


if __name__ == "__main__":
    sys.exit(main())