# sre_env_v1

SRE benchmark sandbox: a stdlib HTTP service with two deliberate config bugs.
- BUG-S1: hardcoded port 8080 ignores config.json service.port=9090
- BUG-S2: /status threshold hardcoded 10 ignores config max_items=5
- logs/incident.log documents the symptoms.

See TASKS.md for the inventory (designer-only). Run: `python service.py`
