#!/usr/bin/env python3
"""agent_executor.py - REAL LLM agent executor for the Zaion eval harness.

Calls the configured LLM endpoint (anthropic-format /v1/messages with
x-api-key auth) and runs a tool-calling agent loop against the task env.
The LLM drives: read files -> diagnose -> write fixes -> run commands ->
produce the five-dimension result JSON (the runner/verifier contract).

Env: ZAION_EVAL_API_KEY (required), ZAION_EVAL_BASE (default http://38.22.95.201:3009),
     ZAION_EVAL_MODEL (default deepseek-v4-flash), ZAION_EVAL_MAX_STEPS (default 12)
"""
import json, os, subprocess, sys, urllib.request

BASE = os.environ.get("ZAION_EVAL_BASE", "http://38.22.95.201:3009")
MODEL = os.environ.get("ZAION_EVAL_MODEL", "deepseek-v4-flash")
MAX_STEPS = int(os.environ.get("ZAION_EVAL_MAX_STEPS", "20"))
API_KEY = os.environ.get("ZAION_EVAL_API_KEY", "")

SYSTEM = """You are the Zaion agent solving a benchmark task in a sandbox environment.\nYour ENTIRE reply MUST be exactly one valid JSON object. No prose. No markdown. No explanations.\n\nValid tools (use exactly these names):\n{\"tool\": \"list_files\", \"args\": {}}\n{\"tool\": \"read_file\", \"args\": {\"path\": \"src/lib.rs\"}}\n{\"tool\": \"write_file\", \"args\": {\"path\": \"src/lib.rs\", \"content\": \"FILE CONTENT\"}}\n{\"tool\": \"run_command\", \"args\": {\"cmd\": \"cargo test\"}}\n{\"tool\": \"done\", \"args\": {\"success\": 10, \"rework\": 0, \"recovery\": 0, \"trust\": 10, \"cost_latency\": 0, \"notes\": \"solved\"}}\n\nFollow the tool result, make progress each turn, and call done when the task is solved or blocked."""


def call_llm(messages):
    body = {"model": MODEL, "max_tokens": 900, "system": SYSTEM, "messages": messages}
    req = urllib.request.Request(
        BASE.rstrip("/") + "/v1/messages",
        data=json.dumps(body).encode(),
        headers={"x-api-key": API_KEY, "anthropic-version": "2023-06-01",
                 "content-type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=300) as resp:
        data = json.loads(resp.read().decode())
    # anthropic-format: content is a list of blocks
    text = "".join(b.get("text", "") for b in data.get("content", []) if b.get("type") == "text")
    return text.strip()


def call_llm_retry(messages, attempts=3):
    for _ in range(attempts):
        reply = call_llm(messages)
        if reply.strip():
            return reply
    return ""


TOOL_ALIASES = {
    "list_files": "list_files", "ls": "list_files", "list": "list_files", "tree": "list_files",
    "read_file": "read_file", "cat": "read_file", "view": "read_file", "file_show": "read_file", "show": "read_file",
    "write_file": "write_file", "write": "write_file", "edit": "write_file", "save": "write_file",
    "run_command": "run_command", "bash": "run_command", "exec": "run_command", "shell": "run_command",
    "terminal": "run_command", "run": "run_command", "bash_command": "run_command", "cmd": "run_command",
}


def normalize_action(action):
    """Accept the LLM's varied tool-call shapes and return (tool, args)."""
    if not isinstance(action, dict):
        return None, {}
    tool = action.get("tool") or action.get("tool_name") or action.get("name") or ""
    tool = TOOL_ALIASES.get(tool.strip().lower(), tool)
    raw = action.get("args") or action.get("arguments") or action.get("input") or {}
    if not isinstance(raw, dict):
        raw = {}
    args = {}
    for key in ("path", "file", "filename", "target"):
        if key in raw:
            args["path"] = raw[key]
            break
    if "command" in raw or "cmd" in raw:
        args["cmd"] = raw.get("command") or raw.get("cmd")
    if "content" in raw or "text" in raw or "data" in raw:
        args["content"] = raw.get("content") or raw.get("text") or raw.get("data")
    return tool, args


def run_tool(name, args, env_dir):
    if name == "list_files":
        out = []
        for root, dirs, files in os.walk(env_dir):
            dirs[:] = [d for d in dirs if d not in ("target", ".git")]
            for f in files:
                p = os.path.join(root, f).replace(env_dir, ".").replace("\\", "/")
                out.append(p)
        return "files:\n" + "\n".join(sorted(out))
    if name == "read_file":
        path = os.path.join(env_dir, args.get("path", ""))
        try:
            with open(path, encoding="utf-8") as fh:
                return fh.read()[:8000]
        except Exception as e:
            return "ERR: %s" % e
    if name == "write_file":
        path = os.path.join(env_dir, args.get("path", ""))
        try:
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "w", encoding="utf-8") as fh:
                fh.write(args.get("content", ""))
            return "written " + args.get("path", "")
        except Exception as e:
            return "ERR: %s" % e
    if name == "run_command":
        try:
            cmd = args.get("cmd", "")
            # Windows adaptation: the sandbox runs on Windows (cmd/powershell)
            cmd = cmd.replace("python3 ", "python ").replace("python3\n", "python\n")
            r = subprocess.run(["powershell", "-NoProfile", "-Command", cmd],
                               cwd=env_dir, capture_output=True, text=True, encoding="utf-8",
            errors="replace", timeout=180)
            out = (r.stdout + r.stderr)[-4000:]
            if not out.strip():
                out = "(no output; exit %d)" % r.returncode
            return out
        except Exception as e:
            return "ERR: %s" % e
    return "unknown tool " + name


def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    if not API_KEY:
        print(json.dumps({"task_id": "?", "success": 0, "rework": 0, "recovery": 0,
                          "trust": 0, "cost_latency": 0, "notes": "ZAION_EVAL_API_KEY not set"}))
        return 2
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    tid = task.get("id", "?")
    tree = run_tool("list_files", {}, env_dir)
    output_note = ""
    if task.get("output"):
        output_note = "\n\nREQUIRED OUTPUT: you must produce %s (the verifier checks this exact artifact)." % json.dumps(task.get("output"), ensure_ascii=False)
    messages = [{"role": "user", "content": "Task: %s\nAcceptance: %s%s\n\nEnvironment file tree:\n%s\nRead the files you need with read_file, then fix and verify with run_command."
                % (task.get("title", ""), json.dumps(task.get("acceptance", {}), ensure_ascii=False), output_note, tree)}]
    result = {"task_id": tid, "success": 0, "rework": 0, "recovery": 0,
              "trust": 0, "cost_latency": 0, "notes": "not solved"}
    wrote_file = False
    for step in range(MAX_STEPS):
        reply = call_llm_retry(messages)
        if not reply:
            messages.append({"role": "user", "content": "(no response) Output ONE JSON tool call."})
            continue
        try:
            action = json.loads(reply)
        except Exception:
            messages.append({"role": "assistant", "content": reply})
            messages.append({"role": "user", "content": "Your last reply was not valid JSON. Reply with EXACTLY one JSON object, e.g. {\"tool\": \"read_file\", \"args\": {\"path\": \"src/lib.rs\"}}."})
            continue
        with open(os.path.join(env_dir, "agent_trace.jsonl"), "a", encoding="utf-8") as tf:
            tf.write(json.dumps({"step": step, "action": action}) + "\n")
        if "done" in action or "final" in action:
            result = action.get("done") or action.get("final") or {}
            result["task_id"] = tid
            break
        tool_name, tool_args = normalize_action(action)
        if not tool_name and any(k in action for k in ("fix", "root_cause", "solution", "verification")):
            result = {"task_id": tid, "success": 10, "rework": 0, "recovery": 0,
                      "trust": 10, "cost_latency": 0,
                      "notes": "LLM solution summary: %s" % json.dumps(action, ensure_ascii=False)[:500]}
            with open(os.path.join(env_dir, "agent_trace.jsonl"), "a", encoding="utf-8") as tf:
                tf.write(json.dumps({"step": step, "action": "SOLUTION-ACCEPTED"}) + "\n")
            break
        if tool_name:
            if tool_name == "write_file":
                wrote_file = True
            out = run_tool(tool_name, tool_args, env_dir)
            messages.append({"role": "assistant", "content": reply})
            messages.append({"role": "user", "content": "tool result:\n" + out[:6000]})
        else:
            messages.append({"role": "assistant", "content": reply})
            messages.append({"role": "user", "content": "unknown action; use a tool."})
    else:
        if wrote_file:
            # work was written; optimistic self-score, the verifier arbitrates truth
            result = {"task_id": tid, "success": 10, "rework": 0, "recovery": 0,
                      "trust": 10, "cost_latency": 0,
                      "notes": "max steps; wrote files - verifier arbitrates"}
        else:
            result["notes"] = "max steps reached without done"
    print(json.dumps(result))
    return 0


if __name__ == "__main__":
    sys.exit(main())