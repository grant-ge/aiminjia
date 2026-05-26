# 断网检测与提示设计

**状态**：Implemented（2026-05-26 完成；自动化测试全过，端到端手测通过）
**日期**：2026-05-26
**实施 PR/分支**：`worktree-feat-network-detection`（基于 main `3d6cc2f2`）

> 本文档记录**最终实现的设计**。设计经过手测阶段调整：
> - banner 形态（而非右上角小角标 + popover）
> - 删除"发送时离线 toast"（banner 已经够强，避免重复打扰）
> - 删除"打开系统网络设置"按钮（不跨平台、收益小）
> - degraded 与 offline 共用同一种视觉（用户层面区分价值低）
>
> 历史决策痕迹见 §11「演进备注」。

## 1. 问题与目标

### 1.1 现状（实施前）

仓库没有专门的网络状态检测：

- 前端只在 updater 处用了一次 `navigator.onLine`（`src/lib/updaterStore.ts:104, 127-128`），其他地方未消费在线/离线信号。
- 后端的 `gateway.rs::is_retryable_error` / `lotus.rs::classify_for_retry` / `chat.rs::classify_llm_error` 已经能在**请求发生时**区分网络错误 vs 服务端错误，但需要用户先发了消息才有反馈。
- 用户在断网时只能看到笼统的"消息发送失败，请检查网络和设置"，分不清是本机断网、VPN 没开、还是 lotus 服务端挂了。

### 1.2 目标

- **持续监控本机到 lotus 网关的连通性**，状态变化时主动告知用户。
- 文案对**普通用户友好**（"网络连接已断开"等白话），技术细节（dns/tls/timeout）只落日志，不进 UI。
- 不阻塞用户操作；离线时仍允许尝试发送，由后端 LLM 错误流处理失败提示。

### 1.3 非目标

- 离线消息排队 / 自动重发。
- 区分"互联网整体断了"vs"只是 lotus 网关挂了"——本期 ping 单一目标即可。
- 多探测目标 / 分层 fallback。

## 2. 总体方案

**Rust 后端常驻探测 + Tauri event 推送状态 + 前端 banner 渲染**。

选择该方案而非纯前端 `fetch` 探测的原因：
- Tauri webview 对跨域 `fetch` 比 Chrome 严格；`HEAD https://ai-tenant.renlijia.com` 大概率被 CORS 拒。
- 复用现有 `reqwest` 配置（已含 `inject_bundled_runtime_path` 等基建）。
- 探测层与 `gateway.rs::is_retryable_error` 同处一个进程，未来可以共享判定逻辑。
- 后端能精确区分 DNS / connect refused / timeout / TLS 失败，前端只接最终结论。

## 3. 模块结构

### 3.1 新增 Rust 模块

```
src-tauri/src/runtime/network/
  mod.rs              ← 对外 re-export
  probe.rs            ← NetworkProbe：tokio interval + reqwest HEAD 探测
  state.rs            ← NetworkStatus 枚举 + NetworkSnapshot 结构

src-tauri/src/transport/tauri_commands/network.rs
                      ← network_get_status / network_force_probe Tauri command

src-tauri/src/lib.rs  ← setup() 内创建独立 TauriRuntimeHost + NetworkProbe，
                        tauri::async_runtime::spawn 启动探测 task
```

**架构约束**（CLAUDE.md 决策 #4）：`runtime/network/` 不得 `use tauri::*`。
- 状态变化通过宿主层的 `RuntimeHost::emit_legacy_event("network:status", ...)` 推到前端。
- `NetworkProbe::new(host)` 接受 `Arc<dyn RuntimeHost>` 注入，自己持有独立的 `reqwest::Client`。
- `NetworkProbe::run() -> impl Future`，**不**自己 spawn；spawn 责任在 transport 层（lib.rs 用 `tauri::async_runtime::spawn`，避免 setup 阶段 tokio 上下文缺失导致的 "no reactor running" panic）。
- `review_network_module.rs` 集成测试守住 `use tauri::` 静态扫描。

### 3.2 新增前端模块

```
src/stores/networkStore.ts         ← Zustand store
src/hooks/useNetworkStatus.ts      ← 订阅 network:status event 写入 store
src/components/shell/NetworkStatusIndicator.tsx ← 顶部全宽 banner
src/lib/tauri.ts                   ← TAURI_EVENTS.NETWORK_STATUS = 'network:status'
                                     + NetworkStatus / NetworkErrorKind / NetworkStatusPayload
                                     + networkGetStatus() / networkForceProbe()
src/i18n/{zh-CN,en-US}.json        ← network namespace（4 个 key）
src/App.tsx                        ← useNetworkStatus() 在顶层；
                                     <NetworkStatusIndicator /> 挂在 TitleBar 下方
```

**不挂载在**：`ChatTopBar` / `PageTopBar`（早期方案，后改为全局 banner 后删除）。

### 3.3 与 spec §3.1 的偏离说明（已记入实现）

spec 初稿要求"通过 `RuntimeEventBus::publish`"发事件，但 `RuntimeEvent` 结构强制带 `SessionId`/`RunId`（`src-tauri/src/runtime/events.rs:218-224`），网络状态是全局事件，不属于任何会话。实现改为：`NetworkProbe` 直接持有 `Arc<dyn RuntimeHost>`，状态变化时调 `host.emit_legacy_event("network:status", payload)`，绕过 `RuntimeEvent` 包装。架构约束（不 use tauri::*）仍然成立。

## 4. NetworkProbe 行为

### 4.1 探测请求

```rust
client.head("https://ai-tenant.renlijia.com")
  .timeout(Duration::from_secs(5))
  .send().await
```

**reqwest client**：使用独立 `reqwest::Client::builder().timeout(Duration::from_secs(5)).build()` 实例，不与 LLM 请求池共用——避免 5s 短超时污染长流式请求。

测试模式（`new_for_test`）额外加 `.no_proxy()`——绕开本机 Clash/v2ray 代理把 127.0.0.1 流量错误外发到代理服务器的问题。生产路径不带 `.no_proxy()`，保留系统代理行为，与其他 reqwest client 一致。

### 4.2 状态分类

| reqwest 结果 | NetworkStatus | 说明 |
|---|---|---|
| 任意 2xx / 3xx / 4xx（含 401/403）| `Online` | TCP+TLS+HTTP 握手成功就算网通；401 不代表网断 |
| 5xx | `ServerDegraded` | 网通但网关异常 |
| timeout / connect error / DNS error / TLS error | `Offline` | 本机无法到达网关 |

`classify_error` 用字符串匹配区分 dns/refused/tls（reqwest 0.12 在 macOS 上 TLS 握手失败归类为 `is_connect()=true`，因此 tls 关键词匹配位于 `is_connect()` 分支内）。

### 4.3 探测节奏

- **基础周期**：30s。
- **首次探测**：app 启动后 `setup()` 末尾立即触发一次，不等首个 tick；interval 使用 `tokio::time::interval_at(now + period, period)` 避免 initial probe + 立即首 tick 的双发。
- **失败退避**：进入 `Offline` 状态后改用 10s 周期；连续 3 次成功后回到 30s（避免 flapping）。
- **MissedTickBehavior::Skip**：避免 macOS 休眠唤醒后一次性补发所有错过的 tick。
- **强制探测**：`network_force_probe` 命令立即触发一次；1 秒内只允许触发 1 次（仅在 `try_send` 成功后推进 `last_force_at_ms`，避免 channel 满时凭空消耗节流窗口）；多余调用返回 `{ "triggered": false }`。

### 4.4 事件发出

只在**状态发生变化时** emit `network:status`（首次 None→Some 也算变化）。状态未变不 emit（减少前端无意义订阅开销）；集成测试 `probe_dedups_unchanged_status` 守住此行为。

### 4.5 退出竞态

tokio task 通过 `tauri::async_runtime::spawn` 启动，进程退出时随 Tauri runtime drop 自然 abort。`run_loop` 的 `force_rx` 用 `Arc<Mutex<Option<Receiver>>>` + `take()` 防止双调用。

### 4.6 探测请求不污染遥测

探测 HEAD 不计入 `TokenUsage`，不进 `RuntimeEventBus` 业务事件流，只走专用 `network:status` legacy event 通道。

## 5. 事件协议

### 5.1 NetworkSnapshot（Rust）

`src-tauri/src/runtime/network/state.rs`：

```rust
#[serde(rename_all = "kebab-case")]
pub enum NetworkStatus { Online, Offline, ServerDegraded }

// snake_case: spec §5.2 requires "connect_refused" (kebab would give "connect-refused")
#[serde(rename_all = "snake_case")]
pub enum NetworkErrorKind { Timeout, Dns, ConnectRefused, Tls, Other }

#[serde(rename_all = "camelCase")]
pub struct NetworkSnapshot {
    pub status: NetworkStatus,
    pub last_check_at_ms: i64,
    pub latency_ms: Option<u32>,            // 仅 Online / ServerDegraded 有值
    pub error_kind: Option<NetworkErrorKind>, // 仅 Offline 有值
}
```

### 5.2 Tauri event

- 名称：`network:status`
- payload TS 类型（`src/lib/tauri.ts`）：

```ts
type NetworkStatus = 'online' | 'offline' | 'server-degraded'
type NetworkErrorKind = 'timeout' | 'dns' | 'connect_refused' | 'tls' | 'other'

interface NetworkStatusPayload {
  status: NetworkStatus
  lastCheckAtMs: number
  latencyMs: number | null
  errorKind: NetworkErrorKind | null
}
```

`TAURI_EVENTS.NETWORK_STATUS = 'network:status'` 常量定义在 `src/lib/tauri.ts`，前端订阅走常量、不用字符串字面量（CLAUDE.md 前端架构要求）。

### 5.3 启动初值

前端 `useNetworkStatus` hook 启动时调用 `network_get_status` Tauri command 拿一次当前快照（可能是 `null`，如 setup 完成但首次 probe 未返回时），避免冷启动时长时间停留在 `unknown`。

## 6. 前端状态与 UI

### 6.1 networkStore（`src/stores/networkStore.ts`）

```ts
interface NetworkStore {
  status: 'online' | 'offline' | 'server-degraded' | 'unknown';
  lastOnlineAt: number | null;          // 仅 'online' 时更新；其它状态保留旧值
  lastCheckAt: number | null;
  latencyMs: number | null;
  errorKind: NetworkErrorKind | null;
  forceProbe: () => Promise<void>;       // 调 invoke('network_force_probe')
  applyEvent: (payload: NetworkStatusPayload) => void;
}
```

初值 `status: 'unknown'`。`unknown` 时 **UI 一律不渲染任何提示**，避免冷启动几百毫秒误报。

### 6.2 useNetworkStatus hook

参照 `useStreaming` 模式，在 `App` 顶层（line 158）挂一次：

1. 启动时 `invoke('network_get_status')` → `applyEvent`（如果 store 已被 `network:status` 事件先于 invoke 响应更新过，此处覆盖也只是同值再写一遍，无副作用）。
2. `listen('network:status')` → `applyEvent`。
3. 卸载时 unlisten（`useEffect` cleanup）；包含 cancelled race 处理（unmount 时如果 listen 还未 resolve，resolve 时立即调用返回的 unlisten）。

### 6.3 NetworkStatusIndicator 组件（**最终设计：顶部全宽 banner**）

挂位置：`src/App.tsx` 内 `AppShell` 的 `<TitleBar />` 与主内容区 `<div className="flex min-h-0 flex-1">` 之间。

| status | 渲染 |
|---|---|
| `online` / `unknown` | 返回 `null`，不占任何 layout 空间 |
| `offline` | 全宽 banner，文案 `network.bannerOfflineText` |
| `server-degraded` | 全宽 banner，文案 `network.bannerDegradedText`（视觉与 offline 相同，仅文案不同） |

Banner 样式（CLAUDE.md UI 规范 - 主题变量优先）：

- 背景 `bg-warning/10`（warning = `#F5A623` 橙色，10% 不透明度，浅橙底）
- 文字 `text-destructive`（红色 `#e7000b`，在浅橙底上对比强）
- 图标 lucide `AlertCircle`，`text-destructive`（与文字同色）
- 重试按钮：`<Button variant="destructive" size="sm">`，实色红底白字
- `border-b border-border` 与主内容区分隔
- `role="alert"` 满足 a11y
- 高度约 36-40px（`px-4 py-2 text-sm`），不占太多空间

**重试按钮交互**：
- 点击 → 立即 `<Loader2 />` spinner 转 + 按钮 disabled
- 监听 `lastCheckAt` 推进（说明新一轮 probe 完成）→ 自动停 spinner
- 5.5s 兜底超时（HEAD 5s + 0.5s buffer）防止 spinner 卡死
- 节流：retrying 期间再点不触发新请求

**未引入** warning 文字色 / Popover 详情面板 / 重连成功 toast 等设计（早期方案，最终未保留——详见 §11 演进备注）。

### 6.4 离线发送提醒（已删除）

> 早期设计：用户在 offline 状态点击发送时弹一个黄色 toast「网络不通 · 消息可能发送失败」。
>
> **手测阶段决定删除**：banner 已经是全宽强提示，再叠 toast 形成"三连发提示"（banner + 离线 toast + 后端 LLM 错误 toast）冗余且嘈杂。`useOfflineSendWarning` hook 和测试已从代码库移除。
>
> 离线时按发送：banner 持续显示；请求照常发出；失败由现有 `classify_llm_error` 流程弹标准错误 toast，与本 feature 无耦合。

## 7. 文案与日志分层

**用户可见层（i18n `network.*`）—— 一律说白话**

`src/i18n/zh-CN.json` 最终 4 个 key：

```json
"network": {
  "retryNow": "重试",
  "bannerOfflineText": "网络连接已断开 · 请检查 WiFi、有线网络或 VPN",
  "bannerDegradedText": "AI 服务暂时无法访问 · 已连接网络，请稍后重试",
  "bannerRetryingText": "正在重新检测网络连接…"
}
```

`src/i18n/en-US.json` 对应：

```json
"network": {
  "retryNow": "Retry",
  "bannerOfflineText": "Network connection lost · Check your Wi-Fi, wired connection, or VPN",
  "bannerDegradedText": "AI service is temporarily unavailable · Network is fine, please retry shortly",
  "bannerRetryingText": "Re-checking network connection…"
}
```

> 早期设计还有 `offlineBadge` / `degradedBadge` / `popoverOfflineTitle/Desc` / `popoverDegradedTitle/Desc` / `lastOnline` / `sendWhileOfflineTitle/Desc` 等 key（对应 popover + 发送 toast 方案），最终删除——只保留 banner 用到的 4 个。

**日志层（落盘 / 开发者可见）—— 保留技术原文**

后端 `log::info!` / `log::warn!` / `log::debug!` 到 `~/.renlijia/logs/`：

```
network probe ok: status=online latency_ms=143       (debug)
network probe degraded: http_status=502 elapsed_ms=812  (warn)
network probe failed: kind=Dns error="failed to lookup address: ai-tenant.renlijia.com" elapsed_ms=4123  (warn)
network status changed -> offline                     (info)
network status changed -> online                      (info)
```

成功路径 30s/次故走 `debug!`，避免一天 720 行 info 噪音；状态变化 + 失败 + degraded 三种保留在 info/warn 级别。

`error_kind` 字段在 `network:status` event payload 中透传到前端 store，store 仅 `console.debug` 打印不渲染——方便用户截图反馈定位，未来扩展时再用。

## 8. 协同与边界

- **与 LLM 错误流的协同**：`server-degraded` 状态**仅**用于挂 banner。LLM 请求失败时 `classify_llm_error` 自己的 toast 路径不变，不会被本 feature 抑制；本 feature 也不再额外弹"发送时" toast（已删除）。
- **不引入 warning 主题色变量新增**：`bg-warning` / `text-warning` 复用 `globals.css` 已定义的 `--color-semantic-orange`（`#F5A623`），不新增主题变量。
- **不做"打开系统网络设置"按钮**：跨平台路径不同、收益小。
- **不引入离线消息队列**：用户在离线时按发送，请求照常发出，由现有 `gateway.rs` 重试机制和 `classify_llm_error` 处理失败。
- **不替换现有 `sendFailedDesc` 文案**：那条文案是 LLM 实际失败后的 toast，不受本 feature 影响。

## 9. 测试策略

| 层 | 文件 | 用例数 | 覆盖 |
|---|---|---|---|
| Rust 单元 | `src-tauri/src/runtime/network/state.rs` `#[cfg(test)]` | 3 | 三种类型 serde 序列化（kebab / snake / camelCase） |
| Rust 单元 | `src-tauri/src/runtime/network/probe.rs` `#[cfg(test)]` | 9 | classify_response 200/401/500/502；force_probe 1s 节流；next_interval_period 退避节奏 4 种 case |
| Rust 集成 | `src-tauri/tests/network_probe_integration_test.rs` | 4 | 用本地 TcpListener stub 替代 wiremock（后者对 HEAD 请求有 bug）：200→online / 503→degraded / connect-refused→offline / 状态去重不重复 emit |
| Rust 架构回归 | `src-tauri/tests/review_network_module.rs` | 1 | `runtime/network/**.rs` 不含 `use tauri::` 字样（守 CLAUDE.md 决策 #4）|
| 前端单元 | `src/stores/networkStore.test.ts` | 4 | event payload → store state；lastOnlineAt 仅 online 更新 |
| 前端组件 | `src/components/shell/__tests__/NetworkStatusIndicator.test.tsx` | 5 | online/unknown 不渲染；offline/degraded 渲染对应文案；点重试调 forceProbe |

总计 **26 个自动化测试，全部通过**；既有 223 个前端测试无回归。

### 9.1 手测脚本（已验证）

1. `pnpm tauri:dev` 启动，~1–2s 内顶部不出现 banner（status=unknown 时不渲染） ✓
2. 关 WiFi → 30s 内顶部出现红字浅橙底 banner「网络连接已断开 …」 ✓
3. 点击 banner 右侧「重试」按钮 → 文案变「正在重新检测网络连接…」 + 按钮 spinner + disabled ✓
4. 重试 5.5s 内 probe 完成 → 自动恢复原文案；连续点 5 次 1 秒内只触发 1 次 ✓
5. 恢复 WiFi → 10s 内 banner 消失 ✓
6. 模拟 5xx（指 PROBE_URL 到 503 stub）→ degraded 文案；banner 视觉与 offline 完全相同（仅文字不同） ✓

## 10. 风险与缓解

| 风险 | 缓解 |
|---|---|
| reqwest 5s 超时与 LLM 长流式请求互相影响 | 独立 `reqwest::Client` 实例，不复用 |
| Tauri setup() 不在 tokio runtime → tokio::spawn panic | 用 `tauri::async_runtime::spawn`，spawn 责任在 transport 层 |
| 状态 flapping 抖动 UI | 失败退避 + 连续 3 次成功才回 30s |
| macOS 休眠唤醒一次性补发 N 次 tick | `MissedTickBehavior::Skip` |
| 前端冷启动几百毫秒 banner 闪烁 | 初值 `unknown` 时一律不渲染；启动后 `network_get_status` 拿初值 |
| 本机系统代理（Clash/v2ray）污染 127.0.0.1 探测 | 测试 client 加 `.no_proxy()`；生产 client 不加（保留与 LLM 请求一致的代理行为） |
| 重试按钮无反馈、用户连点导致暴风请求 | spinner + disabled + 后端 1s 节流 + 前端 5.5s 兜底超时 |

## 11. 演进备注（手测后调整）

记录设计与实现之间的差异，避免后人误以为代码偏离 spec：

1. **角标 → banner**：早期 UI 是右上角小红角标 + 点击展开 Popover 详情。手测发现"交互太弱，用户容易忽略"，参考钉钉断网 banner 改为全宽 banner。删除原 `Popover` 用法、`@/components/ui/popover` 仅本 feature 不再依赖（但仍由其它 feature 使用）。
2. **删除离线发送 toast**：早期有 `useOfflineSendWarning` hook 在发送时弹黄色 toast。banner 上线后冗余，删除 hook + 测试 + 4 个 i18n key（`sendWhileOfflineTitle/Desc` + `offlineBadge/degradedBadge`）。`useChat.ts` 的发送路径回到无网络相关 hook。
3. **删除 popover 详情**：早期 banner 旁边还有 popover 显示 `lastOnline` 时间。最终决定 banner 直接给出操作（重试按钮），不再有 popover；删除 5 个 popover 相关 i18n key。`lastOnlineAt` store 字段保留——给重试按钮的"完成判断"用（监听 `lastCheckAt` 推进）。
4. **warning 主题色**：早期想新加 `--warning` 变量。后发现 `globals.css` 已经有 `--color-semantic-orange`（注册为 `text-warning` / `bg-warning`），直接复用，不改 globals.css。
5. **banner 文字色**：早期是橙色文字（`text-warning`），在浅橙底上对比度低；最终改用 `text-destructive`（红字），仿钉钉。
6. **重试按钮 variant**：早期是 ghost variant + 手覆盖 hover 样式（实测 hover/focus 状态都不对）。最终改用仓库标准 `<Button variant="destructive">`，实色红��白字，hover `brightness-110`，与"danger button"惯例一致。
7. **重试 loading 状态**：spec 初版无此项。手测发现按钮无反馈"很难受"，加 `Loader2` spinner + disabled + 5.5s 兜底 + lastCheckAt 监听自动停。

历史决策的 i18n key 列表（删除）：`offlineBadge` / `degradedBadge` / `popoverOfflineTitle` / `popoverOfflineDesc` / `popoverDegradedTitle` / `popoverDegradedDesc` / `lastOnline` / `sendWhileOfflineTitle` / `sendWhileOfflineDesc`。

## 12. 不做的事（最终）

- 离线消息队列 / 自动重发
- 多目标分层探测（lotus 失败再 ping 公共 DNS）
- 打开系统网络设置按钮
- 新增 warning 主题色变量
- 替换现有 `sendFailedDesc` / LLM 错误流的文案
- 把 `errorKind` 暴露给用户 UI（仅 console.debug + 日志保留）
- 离线发送预警 toast（手测后删除）
- 顶栏小角标 + popover 详情（手测后删除）
- "上次连接成功" 时间展示（手测后删除，但 store 字段保留给重试按钮用）
