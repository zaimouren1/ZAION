#!/usr/bin/env python3
"""channel_sim.py - channel simulation endpoint for Zaion 300-task eval.

Mocks a Telegram Bot API subset + webhook delivery endpoint, with an inspectable
state file so the harness can verify agent behavior.

Endpoints:
  POST /sim/queue                    queue an incoming update (JSON body)
  POST /sim/reset                    reset state
  POST /bot<token>/getUpdates        pop queued updates (limit=N)
  POST /bot<token>/sendMessage       append reply to sent log
  GET  /bot<token>/getMe             bot identity
  POST /webhook/<token>              webhook delivery (honors ?fail=N: respond 500 N times)
  GET  /sim/state                    full sim state (for verification)

Usage: python channel_sim.py --port 8085 --token TESTTOKEN --state sim_state.json
"""
import argparse, json, os, sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import urlparse, parse_qs


class Sim:
    def __init__(self, state_path, token):
        self.state_path = state_path
        self.token = token
        self.state = {"updates": [], "sent": [], "deliveries": [], "fail_remaining": {}}
        self._load()

    def _load(self):
        if os.path.exists(self.state_path):
            try:
                with open(self.state_path, encoding="utf-8") as fh:
                    self.state.update(json.load(fh))
            except Exception:
                pass

    def save(self):
        with open(self.state_path, "w", encoding="utf-8") as fh:
            json.dump(self.state, fh, ensure_ascii=False, indent=2)


class Handler(BaseHTTPRequestHandler):
    sim = None

    def _json(self, code, obj):
        body = json.dumps(obj, ensure_ascii=False).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _read_body(self):
        length = int(self.headers.get("Content-Length", "0"))
        return self.rfile.read(length).decode("utf-8", "replace")

    def do_GET(self):
        parsed = urlparse(self.path)
        if parsed.path == "/sim/state":
            return self._json(200, self.sim.state)
        if parsed.path == "/bot%s/getMe" % self.sim.token:
            return self._json(200, {"ok": True, "result": {"id": 1, "username": "zaion_test_bot"}})
        self._json(404, {"ok": False, "error": "not found"})

    def do_POST(self):
        parsed = urlparse(self.path)
        q = parse_qs(parsed.query)
        if parsed.path == "/sim/reset":
            self.sim.state = {"updates": [], "sent": [], "deliveries": [], "fail_remaining": {}}
            self.sim.save()
            return self._json(200, {"ok": True})
        if parsed.path == "/sim/queue":
            raw = self._read_body()
            try:
                update = json.loads(raw)
            except Exception:
                update = {"raw": raw}
            self.sim.state["updates"].append(update)
            self.sim.save()
            return self._json(200, {"ok": True, "queued": len(self.sim.state["updates"])})
        if parsed.path == "/bot%s/getUpdates" % self.sim.token:
            limit = int(q.get("limit", ["10"])[0])
            out = self.sim.state["updates"][:limit]
            self.sim.state["updates"] = self.sim.state["updates"][limit:]
            self.sim.save()
            return self._json(200, {"ok": True, "result": out})
        if parsed.path == "/bot%s/sendMessage" % self.sim.token:
            raw = self._read_body()
            try:
                payload = json.loads(raw)
            except Exception:
                payload = {"raw": raw}
            self.sim.state["sent"].append(payload)
            self.sim.save()
            return self._json(200, {"ok": True, "result": {"message_id": len(self.sim.state["sent"])}})
        if parsed.path.startswith("/webhook/"):
            token = parsed.path.split("/")[2]
            fail_n = int(q.get("fail", ["0"])[0])
            remaining = self.sim.state["fail_remaining"].get(token, 0)
            raw = self._read_body()
            self.sim.state["deliveries"].append({"token": token, "payload": raw[:500]})
            if remaining > 0:
                self.sim.state["fail_remaining"][token] = remaining - 1
                self.sim.save()
                return self._json(500, {"ok": False})
            if fail_n > 0 and remaining == 0:
                self.sim.state["fail_remaining"][token] = fail_n
            self.sim.save()
            return self._json(200, {"ok": True})
        self._json(404, {"ok": False, "error": "not found"})

    def log_message(self, fmt, *args):
        sys.stderr.write("[channel_sim] %s\n" % (fmt % args))


def main():
    p = argparse.ArgumentParser(prog="channel_sim")
    p.add_argument("--port", type=int, default=8085)
    p.add_argument("--token", default="TESTTOKEN")
    p.add_argument("--state", default="sim_state.json")
    args = p.parse_args()
    Handler.sim = Sim(args.state, args.token)
    httpd = HTTPServer(("127.0.0.1", args.port), Handler)
    print("channel_sim on :%d token=%s state=%s" % (args.port, args.token, args.state), flush=True)
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
