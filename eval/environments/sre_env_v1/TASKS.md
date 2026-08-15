# sre_env_v1 Defect Inventory (designer-only)

| Bug | Location | Expected fix | Mapped tasks |
|---|---|---|---|
| BUG-S1 | service.py main: hardcoded 8080 | bind config service.port (9090) | ZAION-300-HERO-007, ZAION-300-HERO-008 |
| BUG-S2 | service.py /status: hardcoded 10 | use config service.max_items (5) | ZAION-300-HERO-007 |

Verification:
- fixed: service listens on :9090 (config port); /status with 6 items reports unhealthy (threshold 5)
- unfixed: listens on :8080; /status with 12 items reports healthy
