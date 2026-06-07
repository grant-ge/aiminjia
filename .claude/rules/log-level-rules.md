# 日志规范 — Rust 后端日志系统使用准则

## 一、日志架构

### 技术栈

| 组件 | 作用 |
|------|------|
| `log::*!` 宏 | 所有业务代码的日志写入入口，**不要改成 `tracing::*!`** |
| `tracing` + `tracing-subscriber` | 后端格式化、文件写入、trace 上下文传播 |
| `tracing-appender` | 每日轮转文件写入（`~/.renlijia/logs/renlijia.YYYY-MM-DD`） |
| `tracing-subscriber` default features | 内置 `tracing-log` bridge，自动把 `log::*` 路由到 tracing |

### 日志流转

```
log::info!("...")
    ↓  tracing-log bridge（tracing-subscriber default feature 内置）
tracing event
    ↓  TraceContextLayer（读取 span 的 trace_id / span_id）
AijiaLogFormat（自定义格式化）
    ↓  tracing-appender non-blocking writer
~/.renlijia/logs/renlijia.YYYY-MM-DD
```

### 日志格式

```
[2026-06-07 10:58:50][INFO][module::path:42][trace=<id>] 消息
[2026-06-07 10:58:50][INFO][module::path:42][trace=<id> span=<id>] 消息  ← 子 agent
[2026-06-07 10:58:50][INFO][module::path:42][trace=app-d3a91f2b] 消息    ← 后台/启动
```

- `trace` = run_id（UUID 去掉连字符，32 hex，标识一次完整对话轮次）
- `span` = agent_id 前 16 hex（标识子 agent，OTel span_id 规范）
- 后台日志的 `trace=app-<8hex>` 是进程启动时随机生成的，每次启动不同

### Trace 上下文如何传播

业务代码**不需要**手动传 trace_id。`LogContext` + `tracing::Span` 自动处理：

```rust
// session_runtime.rs — 每个 turn 开始时绑定
let ctx = log_context::LogContext::new(session_id, run_id);
log_context::scoped(ctx, async { /* turn 全部逻辑 */ }).await;

// spawn_subagent.rs — 子 agent 创建子 span
let ctx = log_context::LogContext::new(session_id, run_id)
    .with_agent(agent_id);
log_context::scoped(ctx, async { /* sub-agent 逻辑 */ }).await;
```

`.instrument(span)` 穿透 `tokio::spawn`，子任务自动继承上下文，无需手动 rebind。

---

## 二、禁止触碰的配置

### ❌ 不要在 `run()` / `setup()` 里再初始化任何 logger

`tracing_setup::init()` 在 `run()` 函数最开头（Tauri builder 之前）调用一次。
**不要**在其他地方调用任何以下函数：

```rust
log::set_logger(...)          // ❌
tracing_log::LogTracer::init() // ❌ — tracing-subscriber 内置 bridge 已处理
tracing::subscriber::set_global_default(...) // ❌
env_logger::init()            // ❌
```

**历史教训**：`tracing-subscriber` 的 default features 包含 `tracing-log`，其 `.init()` 内部已调用 `LogTracer::init()`。若手动再调一次，启动时 panic：
```
failed to init log→tracing bridge: SetLoggerError(())
```

### ❌ `tracing_setup::init()` 必须在 Tauri builder 之前调用

Tauri 的 `devtools` feature 在 builder 初始化阶段也会尝试调用 `log::set_logger()`。
必须保证我们先于 Tauri 占住全局 logger，否则出现同样的 `SetLoggerError`。

```rust
// lib.rs run() 中的正确顺序：
pub fn run() {
    crate::tracing_setup::init(&logs_dir);  // ← 必须第一个
    log::set_max_level(...);

    let builder = tauri::Builder::default() // ← Tauri builder 在后
        .plugin(...)
        ...
}
```

### ❌ 不要自己构造 `tracing-log` 依赖

`Cargo.toml` 里**不要**单独加 `tracing-log = "..."` 依赖。`tracing-subscriber` 的 default features 已经包含它，额外添加会造成版本冲突或重复初始化。

---

## 三、日志级别含义

| 级别 | 宏 | 适用场景 |
|------|-----|---------|
| `error` | `log::error!` | 不可恢复的错误、panic、数据损坏 |
| `warn` | `log::warn!` | 可恢复的异常、非致命降级、值得关注但不中断流程的问题 |
| `info` | `log::info!` | **重要的生命周期事件**：启动/关机、用户登录/登出、功能初始化完成、关键配置加载 |
| `debug` | `log::debug!` | 开发/排查专用：每次请求的内部细节、工具调用参数、协议帧、计时统计 |

### 禁止的级别误用

- ❌ **高频循环体里用 `info`**：流式 token 回调、SSE 帧解析、工具描述枚举 → `debug`
- ❌ **带 `-trace` / `-timing` / `-dump` 标记的日志用 `info`** → `debug`
- ❌ **打印完整大对象用 `info`**：工具 description 全文、请求/响应 body → `debug`
- ❌ **每次请求必触发的普通细节用 `info`**：函数进出、中间变量、计数更新 → `debug`

`warn` 不是"不确定该用哪级"的保险盒。判断标准：**运维人员看到这条 warn 需要做什么？** 如果答案是"什么都不用做"，就降到 `debug`。

### 自查清单

1. 每秒 / 每请求触发超过 1 次？→ `debug`
2. 内容是内部变量值 / 中间状态？→ `debug`
3. 标记含 `-trace` / `-timing` / `-dump`？→ `debug`
4. 生产环境有人会主动看这条？没有 → `debug`

### 已修复的历史反例（不要重犯）

- `[stream-timing-be]` 曾用 `info`，每个流式 token 打一条 → 已改 `debug`
- `[tool-desc-trace]` 曾用 `info`，每次请求打印全部工具描述 → 已改 `debug`
- `[get_session_key] using cached session_key` 曾用 `info`，每次 LLM 请求触发 → 已改 `debug`
- `[dingtalk-stream] EVENT/CALLBACK payload=<完整消息体>` 曾用 `info` → 已改 `debug`

---

## 四、运行时修改日志级别

通过设置面板（关于 → 开发者）或 Tauri command 动态调整：

```
error  仅错误（最安静）
warn   警告 + 错误
info   标准（默认）
debug  调试（最详细，每次请求都有大量输出）
```

级别持久化在 `~/.renlijia/global/config.json` 的 `log_level` 字段，重启后自动恢复。

实现入口：`transport/tauri_commands/logging.rs` — `set_log_level` / `get_log_level`。
