# 编码规范 — 日常开发必读约束

> 本文件是 CLAUDE.md「重要架构决策」的详细展开。所有规则均来自真实踩坑，每条都有明确的判断标准和历史背景。

---

## 0. 动手前先分析

**写代码之前，必须先完成以下三步：**

1. **影响范围评估**：本次改动会触及哪些模块？是否存在共享状态、trait 实现、跨模块依赖？
2. **副作用检查**：修改现有函数签名、结构体字段、枚举变体之前，grep 全部调用方，确认逐一更新。
3. **最小改动原则**：只改任务直接要求的代码。不顺手重构，不改相邻不相关的代码，不引入未被要求的抽象。

判断标准：**diff 中每一行改动都能直接追溯到用户需求。** 无法追溯的改动 = 不该出现的改动。

---

## 1. HTTP Client — 禁止裸 new

**规则：** 所有对 Lotus 后端（tenant / ops）发起 HTTP 请求的 client，必须通过 `traced_client()` 构造，禁止裸 `reqwest::Client::new()` / `Client::builder().build()`。

```rust
// ❌ 禁止
let client = reqwest::Client::new();
let client = reqwest::Client::builder().timeout(...).build()?;

// ✅ 正确
let client = crate::tracing_setup::traced_client(
    reqwest::Client::builder().timeout(...).build()?
);
```

**为什么：** `traced_client()` 包装了 `TraceHeaderMiddleware`，每个请求自动注入 `X-Trace-Id` / `X-Span-Id`，响应后自动同步服务端 span 序号。裸 client 发出的请求在服务端日志里无法关联，排查链路断链。

**例外：** 第三方服务（钉钉 / 飞书 / Telegram / OpenAI 等）不在此约束，保持现状。

**历史：** `AuthClient`、`AijiaGatewayV2Provider` 曾用裸 client，导致 LLM 请求在网关侧无 trace 关联。

---

## 2. 编译零告警

**规则：** 提交前必须确认 `cargo check --lib` 和 `pnpm tsc --noEmit` 无任何 `warning`（不只是 error）。

常见告警类型及对应处理：

| 告警 | 处理方式 |
|------|---------|
| `unused import` | 删掉，不加 `#[allow]` |
| `dead_code` | 删掉，或确实需要时加 `#[allow(dead_code)]` 并注释理由 |
| `unused variable` | 前缀 `_` 或删掉 |
| `needless_return` | 删掉 `return` |
| TypeScript `TS2xxx` | 修类型，不用 `// @ts-ignore` |

**禁止用 `#[allow(...)]` / `// eslint-disable` 屏蔽告警来蒙混过关**，除非有明确注释说明为什么这个告警在此处不适用。

---

## 3. 后端地址 — 禁止硬编码域名

**规则：** 所有对 Lotus 服务的地址必须通过 `crate::environment` 模块获取，禁止在代码里写死任何 `renlijia.com` 域名。

```rust
// ❌ 禁止
let url = "https://ai-tenant.renlijia.com/v1/chat";
let url = format!("https://ai-tenant.renlijia.com/{}", path);

// ✅ 正确
let url = format!("{}/v1/chat", crate::environment::tenant_host());
let url = format!("{}/{}", crate::environment::ops_host(), path);
```

**为什么：** release 构建锁定 prod，debug 构建跟随 dev 环境切换器（test / pre / prod）。硬编码域名导致 debug 构建永远打生产，测试环境无法隔离。

详见 `.claude/rules/backend-endpoint-rules.md`。

---

## 4. 日志 — 级别必须匹配语义

**规则：** `log::info!` 仅用于重要生命周期事件（启动、登录、关键配置加载）。以下场景禁止用 `info`：

- 每次请求都触发的路径（消息持久化、工具调用、历史加载）→ `debug`
- 循环体内（流式 token、SSE 帧、WebSocket 消息）→ `debug`
- 带 `-trace` / `-timing` / `-dump` 标记的调试日志 → `debug`
- 打印完整对象内容（请求 body、工具描述全文）→ `debug`

**自查问题：** "运维在凌晨 3 点看到这条日志，需要做什么操作？" 答案是"什么都不用做" → 降到 `debug`。

详见 `.claude/rules/log-level-rules.md`。

---

## 5. 多语言 — 新增文案必须同步两份 i18n

**规则：** 任何面向用户的文案，必须同时在 `src/i18n/zh-CN.json` 和 `src/i18n/en-US.json` 中添加对应 key。

```typescript
// ❌ 禁止：硬编码中文字符串
<div>上传日志</div>
toast({ message: '操作成功' })

// ✅ 正确：通过 t() 引用 i18n key
const { t } = useTranslation()
<div>{t('settings.about.uploadLogs')}</div>
toast({ message: t('common.success') })
```

**检查清单：**
- `zh-CN.json` 加了新 key ✓
- `en-US.json` 加了对应 key ✓（不能只写 `TODO` 或直接复制中文）
- key 命名遵循现有层级结构（`模块.子模块.具体项`）

---

## 6. Trace 传播 — 跨边界必须显式传递

跨越以下边界时，trace 上下文不会自动继承，必须显式处理：

| 边界类型 | 传播方式 | 示例 |
|---------|---------|------|
| `tokio::spawn` / `tauri::async_runtime::spawn` | `.instrument(tracing::Span::current())` | `async move { ... }.instrument(span)` |
| HTTP 请求 | `traced_client()` 自动注入 | 见规则 1 |
| Bash / sh 子进程 | `inject_trace_env(&mut command)` | shell_common 已提供 |
| PowerShell 子进程 | `inject_trace_env(&mut command)` | 同上 |

```rust
// ❌ 禁止：spawn 后 trace 断链
tauri::async_runtime::spawn(async move { ... });

// ✅ 正确：捕获当前 span 传入
let span = tracing::Span::current();
tauri::async_runtime::spawn(async move { ... }.instrument(span));
```

**背景：** auto-title 曾因未传 span 导致与 chat turn 出现两个不同 trace ID，无法关联同一次请求的完整链路。

---

## 7. UI 组件 — 优先复用，禁止重造

写前端 UI 前先 grep `src/components/`，优先复用已有组件：

| 需求 | 使用 |
|------|------|
| 按钮 | `<Button variant="...">` |
| 顶栏 | `<ChatTopBar>` / `<PageTopBar>` |
| 对话框 | `<Dialog>` / `<DialogContent>`（自带关闭按钮）|
| 下拉菜单 | `<AppDropdown>` |
| 确认弹窗 | `requestConfirm()` |
| Toast | `useNotificationStore.push({ context: 'toast' })` |

颜色、阴影、图标均须走主题变量，详见 CLAUDE.md 前端 UI 规范第 1–6 条。

---

## 8. 结构体 / 函数修改 — 先查调用方

修改任何 `pub` 函数签名、`pub` 结构体字段、trait 方法时：

```bash
# 先 grep 所有调用方
grep -rn "FunctionName\|StructName" src-tauri/src/ --include="*.rs"
```

确认所有调用方都已同步更新后再提交。遗漏的调用方会在编译时暴露，但跨 crate 边界的 trait 实现可能只在测试或运行时报错。

---

## 自查 Checklist（提交前过一遍）

- [ ] `cargo check --lib` 零 warning
- [ ] `pnpm tsc --noEmit` 零 warning
- [ ] 新增 HTTP client 用 `traced_client()` 包装
- [ ] 新增 `tokio::spawn` 用 `.instrument(Span::current())` 包装
- [ ] 新增子进程 spawn 调用了 `inject_trace_env()`
- [ ] 新增后端 URL 用 `tenant_host()` / `ops_host()` 而非硬编码
- [ ] 新增面向用户文案在 zh-CN 和 en-US 都有对应 key
- [ ] 新增日志级别符合语义（info 只用于生命周期事件）
- [ ] diff 中每行改动都能追溯到本次需求，无多余修改
