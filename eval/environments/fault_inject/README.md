# fault_inject — 故障注入工具包

> 用途: Zaion 300 任务基准的 reliability_security 分类使能器（计划"必测场景"的可执行化）
> 语言: Python 3 stdlib only | 自测: `powershell -File selftest.ps1`

## 工具

| 子命令 | 能力 | 对应必测场景 |
|---|---|---|
| `kill-after <cmd...> --after N --match P` | 运行命令，stdout 匹配 P 达 N 次后 kill（退出 137） | 崩溃发生在 event commit 点 / 进程树取消 |
| `disk-full <path> --fill-mb MB` | 稀疏文件填满配额 | 磁盘满 |
| `reorder --file F [--seed S]` | 打乱 JSONL 事件行 | 乱序事件 |
| `repeat --file F --times N` | 复制事件行 | 重复请求/重放 |
| `tamper --file F --offset N [--xor M]` | 破坏某字节 | 签名篡改 |

## 用法示例

```powershell
# 崩溃于第 3 次 commit
python fault_inject.py kill-after zaion run --after 3 --match "commit"

# 磁盘满（50MB 稀疏填充）
python fault_inject.py disk-full $env:TEMPill --fill-mb 50

# 事件乱序 / 重放
python fault_inject.py reorder --file events.jsonl
python fault_inject.py repeat --file events.jsonl --times 2

# 篡改签名（offset 0 翻转字节）
python fault_inject.py tamper --file signed.bin --offset 0
```

## 状态

全部工具已自测通过（kill-after 退出 137；tamper/reorder/repeat 正常输出）。disk-full 为稀疏写入，注意不要在生产目录使用。
