# 使用 Diagnostics 日志排查问题

本文面向 Codex / 前端排查场景，说明如何用 `logs/metrics.jsonl` 里的 diagnostics 日志快速还原一次 AI 对话、流式输出、工具调用、权限确认、子 agent 执行和前端状态更新链路。

## 日志在哪里

Diagnostics 复用工作区下的 metrics 文件：

```bash
<workspace>/logs/metrics.jsonl
```

如果文件被自动分片，同目录下还会出现历史 shard。应用内的导出、清理、统计入口会同时处理 legacy metrics 和新的 diagnostics 记录。

每一行都是一条紧凑 JSONL。新 diagnostics 行没有旧 metrics 的 `\t✓` 后缀，所以可以直接管道给 `jq`：

```bash
jq -c 'select(.category=="diagnostics")' logs/metrics.jsonl
```

如果要兼容旧 marker 行，可以先去掉后缀：

```bash
sed 's/\t✓$//' logs/metrics.jsonl | jq -c 'select(.category=="diagnostics")'
```

## 核心字段

Diagnostics 设计成机器可查，而不是人眼阅读优先。排查时优先看这些顶层字段：

| 字段 | 含义 | 常见用途 |
| --- | --- | --- |
| `ts` | UTC wall-clock 时间 | 和用户报障时间、后端日志对齐 |
| `seq` | 当前进程内递增序号 | 同一进程内辅助排序 |
| `category` | 固定为 `diagnostics` | 从 metrics 混合文件中过滤诊断事件 |
| `source` | `frontend` 或 `backend` | 判断事件来自前端还是后端 |
| `level` | `debug` / `info` / `warn` / `error` | 快速找错误和告警 |
| `event` | 点分隔事件名 | 查询某类行为，例如 `tool.execute.failed` |
| `ok` | 操作是否成功 | 快速区分 completed / failed |
| `conversationId` | 会话 ID | 还原单个会话链路 |
| `runId` | 单次 agent run ID | 还原一次模型运行链路 |
| `messageId` / `clientMessageId` | 消息 ID | 排查 optimistic message、消息替换、丢消息 |
| `toolCallId` | 工具调用 ID | 串联工具执行和权限链路 |
| `agentId` | 子 agent ID | 排查 subagent 生命周期 |
| `interactionId` | 用户交互 ID | 排查 ask/interaction 是否卡住 |
| `taskId` | 前端 task/后台任务 ID | 排查任务通知和状态变化 |
| `command` | IPC / 后端命令名 | 排查具体命令入口 |
| `durationMs` | 单个操作耗时 | 定位慢操作 |
| `elapsedMs` | 前端从 app 启动到事件发生的耗时 | 前端本地时序辅助字段 |
| `error` | 错误摘要，已脱敏 | 快速定位失败原因 |
| `payload` | 事件上下文摘要，已脱敏 | 看事件相关的轻量上下文 |

注意：只保留 `ts`，没有重复写 `localTime`。需要本地时区时，在查询阶段转换。

## 快速定位流程

### 1. 先确认日志里有什么

```bash
jq -r 'select(.category=="diagnostics") | .event' logs/metrics.jsonl | sort | uniq -c | sort -nr | head -50
```

看最近 50 条 diagnostics：

```bash
tail -200 logs/metrics.jsonl | sed 's/\t✓$//' | jq -c 'select(.category=="diagnostics") | {ts,seq,source,level,event,conversationId,runId,ok,error}' | tail -50
```

### 2. 用 conversationId 收敛范围

用户通常只能描述“某个会话卡住/报错”。先找到这个会话的完整链路：

```bash
CONV=conv_123
jq -c --arg conv "$CONV" '
  select(.category=="diagnostics" and .conversationId==$conv)
  | {ts,seq,source,level,event,runId,messageId,clientMessageId,toolCallId,agentId,interactionId,command,durationMs,ok,error}
' logs/metrics.jsonl
```

如果不知道 `conversationId`，可以先按报障时间附近看 `chat.submit.started`、`backend.command.started`、`turn.started`：

```bash
jq -c '
  select(.category=="diagnostics" and (.event=="chat.submit.started" or .event=="backend.command.started" or .event=="turn.started"))
  | {ts,seq,source,event,conversationId,runId,command,payload}
' logs/metrics.jsonl | tail -100
```

### 3. 找到 runId 后还原一次模型运行

```bash
RUN=run_123
jq -c --arg run "$RUN" '
  select(.category=="diagnostics" and .runId==$run)
  | {ts,seq,source,level,event,conversationId,toolCallId,agentId,interactionId,durationMs,ok,error,payload}
' logs/metrics.jsonl
```

只看时间线摘要：

```bash
jq -c --arg run "$RUN" '
  select(.category=="diagnostics" and .runId==$run)
  | {ts,seq,source,event,durationMs,ok,error}
' logs/metrics.jsonl
```

### 4. 先查 error / warn

```bash
jq -c 'select(.category=="diagnostics" and (.level=="error" or .level=="warn" or .ok==false)) | {ts,seq,source,event,conversationId,runId,command,error,payload}' logs/metrics.jsonl
```

某个会话的错误：

```bash
CONV=conv_123
jq -c --arg conv "$CONV" '
  select(.category=="diagnostics" and .conversationId==$conv and (.level=="error" or .level=="warn" or .ok==false))
  | {ts,seq,source,event,runId,command,error,payload}
' logs/metrics.jsonl
```

## 常见问题排查配方

### 用户点击发送后没有响应

目标：判断卡在前端提交、IPC、后端 command、turn 初始化，还是流式事件返回。

```bash
CONV=conv_123
jq -c --arg conv "$CONV" '
  select(.category=="diagnostics" and .conversationId==$conv and (
    .event|test("chat.submit|ipc.invoke|backend.command|turn.started|turn.config.loaded|turn.history.loaded|streaming.delta|streaming.done|streaming.error")
  ))
  | {ts,seq,source,event,command,runId,durationMs,ok,error,payload}
' logs/metrics.jsonl
```

判断方式：

- 有 `chat.submit.started`，没有 `ipc.invoke.started`：前端 action 或参数构造阶段异常。
- 有 `ipc.invoke.started`，没有 `backend.command.started`：Tauri invoke 或命令名/参数绑定问题。
- 有 `backend.command.started`，没有 `turn.started`：后端 command 到 runtime 入口之间的问题。
- 有 `turn.started`，没有 `streaming.delta.received`：模型调用、历史加载、权限或工具前置阶段卡住。
- 有 `streaming.error.received` 或 `turn.failed`：直接看 `error` 和 `payload`。

### 流式输出卡住或前端没刷新

```bash
CONV=conv_123
jq -c --arg conv "$CONV" '
  select(.category=="diagnostics" and .conversationId==$conv and (
    .event=="streaming.delta.received" or
    .event=="streaming.delta.flushed" or
    .event=="streaming.done.received" or
    .event=="streaming.error.received" or
    .event=="streaming.watchdog.stale_detected" or
    .event=="store.streaming.append" or
    .event=="store.streaming.clear"
  ))
  | {ts,seq,source,event,runId,messageId,durationMs,ok,error,payload}
' logs/metrics.jsonl
```

重点看：

- 后端是否持续发事件：`event.emit.completed`。
- 前端是否收到事件：`event.received`，payload 里有 `eventName`。
- 前端 handler 是否执行完成：`event.handler.completed`。
- store 是否写入：`store.streaming.append` / `store.messages.upsert`。
- 是否触发 watchdog：`streaming.watchdog.stale_detected`。

对比后端发出和前端收到：

```bash
RUN=run_123
jq -c --arg run "$RUN" '
  select(.category=="diagnostics" and .runId==$run and (.event=="event.emit.completed" or .event=="event.received" or .event=="event.handler.failed"))
  | {ts,seq,source,event,ok,error,payload}
' logs/metrics.jsonl
```

### 工具调用失败

```bash
jq -c '
  select(.category=="diagnostics" and (.event|test("tool\\.")))
  | {ts,seq,source,event,conversationId,runId,toolCallId,durationMs,ok,error,payload}
' logs/metrics.jsonl
```

只看失败：

```bash
jq -c 'select(.category=="diagnostics" and .event=="tool.execute.failed") | {ts,seq,conversationId,runId,toolCallId,error,payload}' logs/metrics.jsonl
```

排查顺序：

1. `tool.permission.*`：是否被权限策略挡住。
2. `permission.ask.received`：前端是否收到授权请求。
3. `permission.resolve.*`：用户同意/拒绝/取消是否回到后端。
4. `tool.execute.started/completed/failed`：工具真实执行阶段。
5. `tool.result.*` 或 `tool.round.*`：工具结果是否回到模型 loop。

### 权限弹窗没出现或点击后无效

```bash
CONV=conv_123
jq -c --arg conv "$CONV" '
  select(.category=="diagnostics" and .conversationId==$conv and (
    .event|test("permission|interaction|event.handler")
  ))
  | {ts,seq,source,event,toolCallId,interactionId,ok,error,payload}
' logs/metrics.jsonl
```

判断方式：

- 后端有权限请求但前端没有 `permission.ask.received`：事件转发或监听链路问题。
- 前端有 `permission.ask.received` 但没有 `permission.resolve.started`：UI 没触发提交。
- 有 `permission.resolve.started` 但没有 `permission.resolve.completed`：IPC 或后端 resolve 失败。
- 有 `event.handler.failed`：前端事件 handler 抛错。

### 子 agent 没有通知、状态卡住

```bash
jq -c '
  select(.category=="diagnostics" and (.event|test("agent|subagent|task")))
  | {ts,seq,source,event,conversationId,runId,agentId,taskId,durationMs,ok,error,payload}
' logs/metrics.jsonl
```

重点看：

- 是否有 `agent.spawn.*` / `subagent.*` 的 started 和 completed/failed。
- 是否有 `task:status-changed` 对应的 `event.received`。
- 子 agent 完成后是否出现 `agent.idle.received`。
- 父会话是否被错误清理 busy 状态：看 `store.busy.add/remove`。

### 前端状态和后端事件不一致

先看后端是否发了事件，再看前端是否收到、handler 是否成功、store 是否更新：

```bash
RUN=run_123
jq -c --arg run "$RUN" '
  select(.category=="diagnostics" and .runId==$run and (
    .event=="event.emit.completed" or
    .event=="event.received" or
    .event=="event.handler.started" or
    .event=="event.handler.completed" or
    .event=="event.handler.failed" or
    (.event|test("store\\."))
  ))
  | {ts,seq,source,event,messageId,clientMessageId,toolCallId,ok,error,payload}
' logs/metrics.jsonl
```

典型结论：

- 只有后端 `event.emit.completed`，没有前端 `event.received`：事件通道/监听注册问题。
- 有 `event.received`，没有 `event.handler.completed`：handler 中断。
- 有 `event.handler.completed`，没有对应 `store.*`：handler 没有写目标 store，或条件分支跳过。
- 有 `store.*`，UI 仍异常：继续查组件读取 store 或 selector。

## 用 rg 做粗筛

`rg` 适合先快速缩小范围，再交给 `jq` 精筛。

```bash
rg '"conversationId":"conv_123"' logs/metrics.jsonl
rg '"runId":"run_123"' logs/metrics.jsonl
rg '"level":"error"|"ok":false' logs/metrics.jsonl
rg '"event":"tool.execute.failed"' logs/metrics.jsonl
rg '"toolCallId":"tc_123"' logs/metrics.jsonl
```

如果日志量很大，可以先用 `rg` 再管道给 `jq`：

```bash
rg '"runId":"run_123"' logs/metrics.jsonl | jq -c 'select(.category=="diagnostics") | {ts,seq,source,event,ok,error}'
```

## 按本地时间查看

日志只存 UTC `ts`。本地排查时可以用 `jq` 转换：

```bash
jq -c '
  select(.category=="diagnostics")
  | .local = (.ts | fromdateiso8601 | strflocaltime("%Y-%m-%d %H:%M:%S"))
  | {local,ts,seq,source,event,conversationId,runId,ok,error}
' logs/metrics.jsonl
```

如果 `fromdateiso8601` 对毫秒格式不兼容，可以先去掉毫秒：

```bash
jq -c '
  select(.category=="diagnostics")
  | .tsNoMs = (.ts | sub("\\.[0-9]{3}Z$"; "Z"))
  | .local = (.tsNoMs | fromdateiso8601 | strflocaltime("%Y-%m-%d %H:%M:%S"))
  | {local,ts,seq,source,event,conversationId,runId,ok,error}
' logs/metrics.jsonl
```

## 排查时的推荐顺序

1. 用 `conversationId` 或报障时间圈定范围。
2. 找 `level=="error"`、`level=="warn"`、`ok==false`。
3. 找 `runId`，把一次 turn 的 backend/frontend 时间线串起来。
4. 对比 `event.emit.completed` 和 `event.received`，确认后端事件是否到达前端。
5. 对比 `event.handler.completed` 和 `store.*`，确认前端 handler 是否改了状态。
6. 查 `durationMs`，定位慢 command、慢 tool、慢 handler。
7. 查 `payload`，确认分支条件、摘要、计数、eventName 等上下文。

## 事件命名速查

常见事件前缀：

| 前缀 | 含义 |
| --- | --- |
| `chat.*` | 前端发起聊天相关动作 |
| `conversation.*` | 前端会话创建、切换、删除、归档、重命名 |
| `ipc.invoke.*` | 前端调用 Tauri command |
| `backend.command.*` | 后端 Tauri command 入口 |
| `turn.*` | 后端一次 agent turn 生命周期 |
| `event.emit.*` | 后端向前端 emit 事件 |
| `event.received` | 前端收到 Tauri 事件 |
| `event.handler.*` | 前端事件 handler 执行 |
| `streaming.*` | 流式输出相关事件 |
| `store.*` | 前端 Zustand store 关键 mutation |
| `tool.*` | 工具权限、执行和结果链路 |
| `permission.*` | 权限请求和 resolve 链路 |
| `interaction.*` | 用户交互请求和 resolve 链路 |
| `agent.*` / `subagent.*` | 子 agent / 后台 agent 生命周期 |
| `diagnostics.*` | diagnostics 自身转发或记录问题 |

## 脱敏和 payload 限制

Diagnostics 会脱敏常见 secret：API key、token、cookie、authorization、bearer token、`sk-` 前缀、password 类字段等。

前端 payload 会做摘要，避免把超大对象完整写入日志。流式 delta 默认应优先记录元信息和长度，不依赖正文内容排查。排查正文内容时，应结合消息表或业务数据源，不要假设 diagnostics 一定包含完整用户输入或模型输出。

## 一条命令生成排查摘要

下面命令适合先贴给 Codex，看一段会话的关键时间线：

```bash
CONV=conv_123
sed 's/\t✓$//' logs/metrics.jsonl \
  | jq -c --arg conv "$CONV" '
    select(.category=="diagnostics" and .conversationId==$conv)
    | {
        ts,
        seq,
        source,
        level,
        event,
        runId,
        command,
        messageId,
        clientMessageId,
        toolCallId,
        agentId,
        interactionId,
        taskId,
        durationMs,
        ok,
        error,
        payload
      }
  '
```

如果输出太多，先只给异常和关键事件：

```bash
CONV=conv_123
sed 's/\t✓$//' logs/metrics.jsonl \
  | jq -c --arg conv "$CONV" '
    select(.category=="diagnostics" and .conversationId==$conv)
    | select((.level=="error") or (.level=="warn") or (.ok==false) or (.event|test("chat.submit|backend.command|turn.|tool.|permission.|interaction.|streaming.done|streaming.error|event.handler.failed")))
    | {ts,seq,source,level,event,runId,command,toolCallId,interactionId,durationMs,ok,error,payload}
  '
```
