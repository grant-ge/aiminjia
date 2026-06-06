# 日志关联 ID 诊断与方案

> 诊断日期：2026-06-07
> 背景：日志乱、出问题没法排查。本文先给诊断，再给「请求关联 ID」方案。
> 范围（本次）：① 诊断报告；② 请求关联 ID。日志级别治理（warn 滥用）单列、后续手动调。

## 1. 现状

- 日志后端：`tauri-plugin-log 2.8.0`（底层 `fern`，同步 dispatch）+ `log 0.4` facade，**无 `tracing` 依赖**。
- 初始化：`src-tauri/src/lib.rs:162`，单 Folder target（`renlijia.log`），`LevelFilter::Info`，KeepOne 轮转，5MB。
- 输出格式：`[日期][时间][级别][模块路径] 消息`，例：
  ```
  [2026-06-07][06:39:30][INFO][app_lib::auth] [get_session_key] using cached session_key (...)
  ```

## 2. 病灶（带数据）

| # | 问题 | 证据 | 排查影响 |
|---|---|---|---|
| 1 | **无请求关联 ID（最致命）** | 一次消息流经 `session_runtime → chat_turn_driver → tools → llm/gateway` 十几个模块，每行日志互相独立 | 没法 grep 出「某一次请求」的完整链路，只能靠时间戳猜，并发时彻底失效 |
| 2 | 级别金字塔倒挂 | `438 warn > 361 info > 93 error`（`grep log::*!` 统计） | warn 被当普通日志用，真告警淹没 |
| 3 | 双重前缀冗余 | `[app_lib::auth] [get_session_key]`——模块路径已有，手写 `[函数名]` 重复 | 噪音，且几十种风格不统一 |

> 本次只解决 #1。#2/#3 留作后续治理。

## 3. 方案：`tokio::task_local!` 注入关联 ID（非 tracing 迁移）

### 为什么不上 `tracing`

全仓 ~950 处 `log::*` 调用点。全量迁到 `tracing` span 是大重构，与本次「加关联 ID」的目标不成比例。

### 选定方案：task-local 上下文 + 自定义日志格式

- 在主链路入口把 `(SessionId, RunId)` 绑进一个 `tokio::task_local!`（subagent 额外带 `AgentId`）。
- 自定义 `tauri-plugin-log` 的 `.format()`，**同步**读取该 task-local，把 `[s=<sid> r=<rid>]` 拼进每行日志。
- **零改动 950 个调用点**：日志文案不动，关联 ID 由 formatter 自动补。

### 可行性验证

- `fern` 的 `Log::log()` 同步执行 format 闭包（在发日志的那个 task 的调用栈里）→ task-local `try_with` 读得到。
- 主 turn 是**直接 await**（`session_runtime.rs:340`，非 spawn）→ scope 自然覆盖整条链路（tools / LLM 都在同一 task）。
- subagent 是单独 `tokio::spawn`（`spawn_subagent.rs:309`）→ task-local **不跨 spawn 传递**，在 spawn 闭包内**重建 scope**（带 agent id）。
- 不在 scope 内的日志（启动、idle、后台线程）→ `prefix()` 返回空串，行内不加任何东西，保持干净。

### 改动清单

| 文件 | 改动 |
|---|---|
| `src-tauri/src/log_context.rs`（新增） | `LogContext` + `tokio::task_local!` + `scoped()` + `prefix()` |
| `src-tauri/src/lib.rs` | 声明 `mod log_context`；给 plugin builder 加 `.format()` |
| `src-tauri/src/runtime/session_runtime.rs` | `run_chat_request` body 包进 `log_context::scoped` |
| `src-tauri/src/llm/tool_executor/spawn_subagent.rs` | subagent body 包进带 agent id 的 scope |

### 输出效果（改造后）

```
[2026-06-07][06:39:30][INFO][app_lib::auth][s=sess_abc r=run_def] [get_session_key] using cached session_key
[2026-06-07][06:39:31][INFO][app_lib::runtime::chat::chat_turn_driver][s=sess_abc r=run_def] ...
```
排查时 `grep 'r=run_def' renlijia.log` 即可拉出整条请求链路（含其 subagent，行内多一段 `a=<agent>`）。

兼容性：原有 `[INFO]` / 模块路径位置不变，旧 grep 不受影响；关联段是**新增**维度。
