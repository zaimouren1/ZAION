# M3 产品内 hero 执行指南（2026-08-14 实测）

## 前置

- 已构建二进制：cargo build -p zaion-cli --bin zaion
- API 配置（评测/产品共用端点）：
  - ANTHROPIC_API_KEY=<key>
  - ANTHROPIC_BASE_URL=<endpoint>
  - 产品 anthropic provider 原生 x-api-key（自定义端点自动 openai-compat 工具格式）

## 首次 onboarding（隔离配置）

```
$env:ZAION_HOME = "$env:TEMP\zaion-m3-smoke"
# pipe 模拟 wizard：provider 索引 0(anthropic) / api-key / base-url / model / channels / workspace
$answers = "0`n<key>`n<endpoint>`ndeepseek-v4-flash`n`ndefault"
$answers | zaion onboard
zaion config set model deepseek-v4-flash   # 确保模型覆盖默认
```

## 运行 hero 任务

```
$env:ZAION_TOOL_SUBSET = "fs_read,fs_write,fs_list,fs_search,shell_exec"  # 精简工具保持工具倾向
$env:ANTHROPIC_API_KEY = <key>
$env:ANTHROPIC_BASE_URL = <endpoint>
cd <任务工作目录>
zaion wake <principal-id> "Run cargo test and fix the failing tests."
```

## 已验证场景（0.5-1.2min）

- 代码修复（sandbox 3 缺陷 → cargo 6/6）
- SRE 配置修复（端口/阈值）
- 崩溃恢复（journal 应用 + committed）
- 安全验证（收据签名检测 + 报告）


## 简化：zaion hero 命令（第 176-177 轮产品化）

```
zaion hero <principal-id> "Run cargo test and fix the failing tests."
```
- 自动设置核心工具子集（fs_read/fs_write/fs_list/fs_search/shell_exec）
- 无需手动 ZAION_TOOL_SUBSET
- `zaion hero --help` 查看说明

## 关键机制（M3 实测发现）

| 机制 | 说明 |
|---|---|
| 工具子集（ZAION_TOOL_SUBSET） | 67→5 工具保持 LLM 工具倾向 |
| openai-compat 消息 | 自定义端点 assistant/tool 消息格式 |
| reasoning_content 回传 | deepseek 思考模式多轮要求 |
| MAX_NATIVE_TOOL_TURNS=24 | 复杂任务工具轮数 |
| cancel 标记文件 | 零 IPC 跨进程取消（zaion turn cancel --pid） |