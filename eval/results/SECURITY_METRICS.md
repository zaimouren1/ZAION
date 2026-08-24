# Security Metrics（顶层安全指标）

| 维度 | 对抗测试数 | 穿透数 | Escape Rate | 结论 |
|---|---|---|---|---|
| auth_bypass | 43 | 0 | 0.0% | SAFE (0% escape) |
| rbac | 8 | 0 | 0.0% | SAFE (0% escape) |
| ssrf | 2 | 0 | 0.0% | SAFE (0% escape) |
| secret_leak | 7 | 0 | 0.0% | SAFE (0% escape) |

Security Escape Rate = 穿透数 / 对抗测试数（本应被阻断的攻击中，实际穿透的比例）。0% = 全部阻断。
