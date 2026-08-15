#!/usr/bin/env python3
"""sre_env_v1 service - benchmark sandbox with a deliberate config bug.

BUG-S1: the service reads config.json 'port' but binds a hardcoded 8080.
BUG-S2: the /health threshold check uses hardcoded 10 instead of config 'max_items'.
NOTE FOR DESIGNERS: bugs are intentional; TASKS.md holds the inventory.
"""
import json, os, sys
from http.server import BaseHTTPRequestHandler, HTTPServer

CONFIG_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "config.json")


def load_config():
    with open(CONFIG_PATH, encoding="utf-8") as fh:
        return json.load(fh)


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/health":
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b'{"status":"ok"}')
        elif self.path == "/status":
            cfg = load_config()
            items = int(self.headers.get("X-Items", "0"))
            # BUG-S2: hardcoded threshold ignores config max_items
            healthy = items <= 10
            body = json.dumps({"healthy": healthy, "items": items}).encode()
            self.send_response(200)
            self.end_headers()
            self.wfile.write(body)
        else:
            self.send_response(404)
            self.end_headers()

    def log_message(self, fmt, *args):
        sys.stderr.write("[sre] %s\n" % (fmt % args))


def main():
    cfg = load_config()
    port = cfg.get("service", {}).get("port", 8080)
    # BUG-S1: hardcoded port overrides config
    httpd = HTTPServer(("127.0.0.1", 8080), Handler)
    print("sre service listening on :%d (config says :%d)" % (8080, port), flush=True)
    httpd.serve_forever()


if __name__ == "__main__":
    main()
