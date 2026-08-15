# channel_sim — 渠道模拟端点

> 用途: Zaion 300 任务基准 channels 分类的可执行环境（Telegram Bot API 子集 + webhook 投递）
> 自测: `powershell -File test_sim.ps1`

## 端点

| 端点 | 方法 | 用途 |
|---|---|---|
| `/sim/queue` | POST | 入队一条入站 update（JSON body） |
| `/sim/reset` | POST | 重置状态 |
| `/bot<token>/getUpdates` | POST | 取队列中的 update（limit=N） |
| `/bot<token>/sendMessage` | POST | 记录 agent 的回复（sent 日志） |
| `/bot<token>/getMe` | GET | bot 身份 |
| `/webhook/<token>` | POST | webhook 投递（?fail=N 模拟前 N 次失败） |
| `/sim/state` | GET | 全状态（供验证器检查：sent/deliveries/updates） |

## 用法

```powershell
python channel_sim.py --port 8085 --token TESTTOKEN --state sim_state.json
# 入队消息
Invoke-RestMethod -Uri "http://127.0.0.1:8085/sim/queue" -Method Post -Body '{"update_id":1,"message":{"text":"hi","chat":{"id":42}}}' -ContentType "application/json"
# agent 轮询并回复（模拟被测 agent 行为）
# 验证: GET /sim/state 检查 sent 里应有回复
```

## 状态

E2E 验证通过（queue → getUpdates 拿到 text → sendMessage 记录 → state 可查）。webhook 端点含 ?fail=N 重试模拟（对应重试/幂等任务）。
