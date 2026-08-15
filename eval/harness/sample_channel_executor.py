#!/usr/bin/env python3
"""sample_channel_executor.py - demonstrates a channel task execution.

Flow (what the real agent would do):
  1. start channel_sim in the env
  2. queue an incoming update
  3. poll getUpdates, process the message
  4. reply via sendMessage
  5. leave sim_state.json for the verifier

Used with: python runner.py --run ZAION-300-CH-001 --executor "python eval/harness/sample_channel_executor.py" --env DIR
"""
import json, os, subprocess, sys, time, urllib.request

PORT = 8090
TOKEN = "TESTTOKEN"


def http_post(url, body=None, headers=None):
    data = json.dumps(body).encode() if body is not None else b""
    h = {"Content-Type": "application/json"}
    if headers:
        h.update(headers)
    req = urllib.request.Request(url, data=data, headers=h)
    with urllib.request.urlopen(req, timeout=5) as resp:
        return json.loads(resp.read())


def main():
    env_dir, task_file = sys.argv[1], sys.argv[2]
    with open(task_file, encoding="utf-8") as fh:
        task = json.load(fh)
    sim = os.path.join(env_dir, "channel_sim.py")
    state_path = os.path.join(env_dir, "sim_state.json")
    proc = subprocess.Popen(["python", sim, "--port", str(PORT), "--token", TOKEN, "--state", state_path],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        time.sleep(1.5)
        base = "http://127.0.0.1:%d" % PORT
        http_post(base + "/sim/reset")
        http_post(base + "/sim/queue", {"update_id": 1, "message": {"text": "hello zaion", "chat": {"id": 42}}})
        upd = http_post(base + "/bot%s/getUpdates" % TOKEN)
        if upd.get("result"):
            http_post(base + "/bot%s/sendMessage" % TOKEN, {"chat_id": 42, "text": "reply from zaion"})
        time.sleep(0.5)
        # result for the runner
        with open(state_path, encoding="utf-8") as fh:
            state = json.load(fh)
        ok = len(state.get("sent", [])) >= 1
        print(json.dumps({"task_id": task["id"], "success": 10 if ok else 0, "rework": 0,
                          "recovery": 0, "trust": 10, "cost_latency": 0,
                          "evidence_path": state_path,
                          "notes": "channel flow: %d replies" % len(state.get("sent", []))}))
    finally:
        proc.kill()


if __name__ == "__main__":
    main()
