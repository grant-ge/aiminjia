# 断网检测与提示设计

**状态**：Draft（待评审）
**日期**：2026-05-26

## 1. 问题与目标

### 1.1 现状

仓库目前没有专门的网络状态检测：

- 前端只在 updater 处用了一次 `navigator.onLine`（`src/lib/updaterStore.ts:104, 127-128`），其他地方未消费在线/离线信号。
- 后端的 `gateway.rs::is_retryable_error` / `lotus.rs::classify_for_retry` / `chat.rs::classify_llm_error` 已经能在**请求发生时**区分网络错误 vs 服务端错误，但需要用户先发了消息才有反馈。
- 用户在断网时只能看到笼统的"消息发送失败，请检查网络和设置"（i18n key `sendFailedDesc`），分不清是本机断网、VPN 没开、还是 lotus 服务端挂了。

### 1.2 目标

- **持续监控本机到 lotus 网关的连通性**，状态变化时主动告知用户。
- 文案对**普通用户友好**（"网络不通"），技术细节（dns/tls/timeout）只落日志，不进 UI。
- 不阻塞用户操作；离线时仍允许尝试发送，仅以 toast 提醒"网络不通，可能失败"。

### 1.3 非目标

- 离线消息排队 / 自动重发：本期不做。
- 区分"互联网整体断了"vs"只是 lotus 网关挂了"：本期 ping 单一目标即可。
- 多探测目标 / 分层 fallback：本期不做。

## 2. 总体方案

**Rust 后端常驻探测 + Tauri event 推送状态 + 前端订阅渲染**。

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

src-tauri/src/transport/tauri_event_adapter.rs
                      ← 增加 RuntimeEvent::NetworkStatusChanged → `network:status` 映射
```

**架构约束**（CLAUDE.md 决策 #4）：`runtime/network/` 不得 `use tauri::*`。状态变化通过 `RuntimeEventBus::publish(RuntimeEvent::NetworkStatusChanged { ... })` 发出；`NetworkProbe` 自己持有一个独立的 `reqwest::Client`（见 §4.1，不与 LLM 请求池共用），shutdown signal 通过 `RuntimeHost` trait 注入（如 trait 当前未提供，则在 trait 上新增一个 `shutdown_signal()` 方法）。

### 3.2 新增前端模块

```
src/stores/networkStore.ts         ← Zustand store
src/hooks/useNetworkStatus.ts      ← 订阅 network:status event 写入 store
src/hooks/useOfflineSendWarning.ts ← 用户发送时检查 store，离线则 toast
src/components/shell/NetworkStatusIndicator.tsx ← 顶栏红点 + popover
src/lib/tauri.ts                   ← TAURI_EVENTS 增加 NetworkStatus = 'network:status'
src/i18n/{zh-CN,en-US}.json        ← 新增 network 命名空间
```

## 4. NetworkProbe 行为

### 4.1 探测请求

```rust
client.head("https://ai-tenant.renlijia.com")
  .timeout(Duration::from_secs(5))
  .send().await
```

**reqwest client**：使用独立的 `reqwest::Client::builder().timeout(Duration::from_secs(5)).build()` 实例，不与 LLM 请求池共用——避免 5s 短超时污染长流式请求。

### 4.2 状态分类

| reqwest 结果 | NetworkStatus | 说明 |
|---|---|---|
| 任意 2xx / 3xx / 4xx（含 401/403）| `Online` | TCP+TLS+HTTP 握手成功就算网通；401 不代表网断 |
| 5xx | `ServerDegraded` | 网通但网关异常 |
| timeout / connect error / DNS error / TLS error | `Offline` | 本机无法到达网关 |

### 4.3 探测节奏

- **基础周期**：30s。
- **首次探测**：app 启动后 `setup()` 末尾立即触发一次，不等首个 tick。
- **失败退避**：进入 `Offline` 状态后改用 10s 周期；连续 3 次成功后回到 30s（避免 flapping）。
- **MissedTickBehavior::Skip**：避免 macOS 休眠唤醒后一次性补发所有错过的 tick。
- **强制探测**：`network_force_probe` 命令立即触发一次，1 秒内只允许触发 1 次，多余调用返回当前 cached 状态。

### 4.4 事件发出

只在**状态发生变化时** emit `RuntimeEvent::NetworkStatusChanged`，状态未变不 emit（减少前端无意义订阅开销）。

### 4.5 退出竞态

tokio task 用 `tokio::select!` 监听 `RuntimeHost` 提供的 shutdown signal（如不存在则随 runtime drop 自然 abort），避免进程退出阻塞。

### 4.6 探测请求不污染遥测

探测 HEAD 不计入 `TokenUsage`，不进 `RuntimeEventBus` 业务事件流，只走专用 `NetworkStatusChanged` 通道。

## 5. 事件协议

### 5.1 RuntimeEvent

`src-tauri/src/runtime/events.rs` 新增：

```rust
pub enum NetworkStatus {
    Online,
    Offline,
    ServerDegraded,
}

pub enum NetworkErrorKind {
    Timeout,
    Dns,
    ConnectRefused,
    Tls,
    Other,
}

RuntimeEvent::NetworkStatusChanged {
    status: NetworkStatus,
    last_check_at_ms: i64,
    latency_ms: Option<u32>,            // 仅 Online 有值
    error_kind: Option<NetworkErrorKind>, // 仅 Offline 有值
}
```

### 5.2 Tauri event

- 名称：`network:status`
- payload TS 类型在 `src/lib/tauri.ts` 与 RuntimeEvent 字段一一对应：

```ts
type NetworkStatus = 'online' | 'offline' | 'server-degraded';
type NetworkErrorKind = 'timeout' | 'dns' | 'connect_refused' | 'tls' | 'other';

interface NetworkStatusPayload {
  status: NetworkStatus;
  lastCheckAtMs: number;
  latencyMs: number | null;
  errorKind: NetworkErrorKind | null;
}
```

`TAURI_EVENTS` 常量增加 `NetworkStatus = 'network:status'`，前端订阅走常量、不用字符串字面量（CLAUDE.md 前端架构要求）。

### 5.3 启动初值

App 启动时前端调用 `network_get_status` Tauri command 拿一次当前快照，避免冷启动时长时间停留在 `unknown`。

## 6. 前端状态与 UI

### 6.1 networkStore

```ts
interface NetworkStore {
  status: 'online' | 'offline' | 'server-degraded' | 'unknown';
  lastOnlineAt: number | null;
  lastCheckAt: number | null;
  latencyMs: number | null;
  errorKind: NetworkErrorKind | null;
  forceProbe: () => Promise<void>;       // 调 invoke('network_force_probe')
  applyEvent: (payload: NetworkStatusPayload) => void; // 由 hook 调用
}
```

初值 `unknown`。`unknown` 时 **UI 一律不渲染任何提示**，避免冷启动几百毫秒误报。

### 6.2 useNetworkStatus hook

参照 `useStreaming` 模式，在 `App.tsx` 顶层挂一次：

1. 启动时 `invoke('network_get_status')` → `applyEvent`。
2. `listen('network:status')` → `applyEvent`。
3. 卸载时 unlisten（`useEffect` cleanup）。

### 6.3 NetworkStatusIndicator 组件

挂位置：`ChatTopBar` / `PageTopBar` 右侧（CLAUDE.md 要求顶栏复用既有组件）。

| status | 渲染 |
|---|---|
| `online` / `unknown` | 不渲染 |
| `offline` | lucide `WifiOff` + `text-destructive`，外层 `bg-destructive/12 rounded-full p-[6px]` |
| `server-degraded` | lucide `CloudOff` + `text-muted-foreground`，外层同上但用 muted 背景 |

点击 → Radix `Popover`（复用 `@/components/ui/popover` 若存在，否则 `@/components/common/AppDropdown`）：

| 字段 | offline | server-degraded |
|---|---|---|
| 标题 | `network.popoverOfflineTitle` | `network.popoverDegradedTitle` |
| 正文 | `network.popoverOfflineDesc` | `network.popoverDegradedDesc` |
| 上次在线 | `network.lastOnline`（formatRelative） | 同左 |
| 操作 | 「重试」按钮 → `forceProbe()` | 同左 |

**不**展示 `errorKind` 文案。`errorKind` 仅写入 store，未来扩展时再用；此处通过 `console.debug` 打到 webview console，方便用户截图反馈。

颜色与图标遵循 CLAUDE.md UI 规范：
- 全部用主题变量（`text-destructive` / `text-muted-foreground`）。
- 不引入 warning 语义色变量，degraded 用 muted 即可。
- lucide 图标颜色由 `currentColor` 驱动，不传 `color`/`stroke` 字面量。

### 6.4 useOfflineSendWarning hook

挂在聊天发送入口（搜 `send_message` 调用方加一行）：

```ts
const onSend = (...) => {
  if (useNetworkStore.getState().status === 'offline') {
    useNotificationStore.getState().push({
      context: 'toast',
      level: 'warning',
      title: t('network.sendWhileOfflineTitle'),
      message: t('network.sendWhileOfflineDesc'),
      autoHide: 6000,
    });
  }
  // 不阻止，原本发送流程继续
  doSend();
};
```

只在 `offline` 时弹。`server-degraded` 时由后端 `classify_llm_error` 路径自己处理失败 toast，避免重复打扰。

## 7. 文案与日志分层

**用户可见层（i18n）—— 一律说"网络不通"**

`src/i18n/zh-CN.json` 新增：

```json
"network": {
  "offlineBadge": "网络不通",
  "degradedBadge": "AI 服务暂时不可用",
  "popoverOfflineTitle": "当前无法连接到网络",
  "popoverOfflineDesc": "请检查 WiFi、有线网络或 VPN 是否正常，然后点击「重试」。",
  "popoverDegradedTitle": "AI 服务暂时无法访问",
  "popoverDegradedDesc": "网络已连通，但 AI 服务端暂时异常，请稍后重试。",
  "lastOnline": "上次连接成功：{{time}}",
  "retryNow": "重试",
  "sendWhileOfflineTitle": "网络不通",
  "sendWhileOfflineDesc": "消息可能发送失败，请检查网络后重试。"
}
```

`src/i18n/en-US.json` 同步：

```json
"network": {
  "offlineBadge": "No network",
  "degradedBadge": "AI service unavailable",
  "popoverOfflineTitle": "Can't connect to the network",
  "popoverOfflineDesc": "Check your Wi-Fi, wired connection, or VPN, then click Retry.",
  "popoverDegradedTitle": "AI service is temporarily unavailable",
  "popoverDegradedDesc": "Your network is fine, but the AI service is having issues. Please retry shortly.",
  "lastOnline": "Last online: {{time}}",
  "retryNow": "Retry",
  "sendWhileOfflineTitle": "No network",
  "sendWhileOfflineDesc": "Sending may fail. Please check your network and retry."
}
```

**日志层（落盘 / 开发者可见）—— 保留技术原文**

后端 `tracing::warn!` / `tracing::info!` 到 `~/.renlijia/logs/`（CLAUDE.md 全局规则约定查日志入口）：

```
network probe failed: kind=dns error="failed to lookup address: ai-tenant.renlijia.com" elapsed_ms=4123
network probe failed: kind=tls error="invalid certificate ..." elapsed_ms=812
network probe failed: kind=timeout error="operation timed out" elapsed_ms=5001
network status changed: offline -> online latency_ms=143
network status changed: online -> server_degraded http_status=502
```

前端 `console.debug` 把 `errorKind` 与 `lastCheckAtMs` 输出，方便用户截图反馈定位。

## 8. 协同与边界

- **与 LLM 错误流的协同**：`server-degraded` 状态**仅**用于状态栏角标。LLM 请求失败时 `classify_llm_error` 自己的 toast 路径不变，不会因为有 degraded 状态而抑制；发送 toast 仅在 `offline` 时触发，不在 degraded 时触发。
- **不引入 warning 主题色变量**：仓库当前没有 `--warning`，本期不新增；degraded 用 `text-muted-foreground` 表达"次要异常"语义。
- **不做"打开系统网络设置"按钮**：跨平台路径不同、收益小，本期 popover 只放「重试」一个按钮。
- **不引入离线消息队列**：用户在离线时按发送，请求照常发出，由现有 `gateway.rs` 重试机制和 `classify_llm_error` 处理失败。
- **不替换现有 `sendFailedDesc` 文案**：那条文案是 LLM 实际失败后的 toast，本设计的 `sendWhileOfflineDesc` 是预防性提示，两者并存且场景不重叠（offline → 预防提示；实际失败 → sendFailedDesc）。

## 9. 测试策略

| 层 | 文件 | 用例 |
|---|---|---|
| Rust 单元 | `src-tauri/src/runtime/network/probe.rs` 内 `#[cfg(test)] mod tests` | classify() 各种 reqwest 错误 → 三态映射；退避节奏（offline 时 10s，连续 3 次成功回 30s）；force probe 1s 节流 |
| Rust 集成 | `src-tauri/tests/network_probe_integration_test.rs` | 用 `wiremock` 起本地 server，模拟 200 / 500 / connection refused，断言事件 emit 顺序与去重（状态未变不 emit） |
| Rust 架构回归 | `src-tauri/tests/review_network_module.rs` | grep `runtime/network/**.rs` 不含 `use tauri::` 字样（守 CLAUDE.md 决策 #4）|
| 前端单元 | `src/stores/networkStore.test.ts` | event payload → store state 迁移；forceProbe 调用 invoke；unknown → online 不弹任何提示 |
| 前端集成 | `src/hooks/useOfflineSendWarning.test.tsx` | offline 发送 → toast push 一次；online 发送 → 不弹；server-degraded 发送 → 不弹 |
| 前端组件 | `src/components/shell/__tests__/NetworkStatusIndicator.test.tsx` | online/unknown 不渲染；offline 渲染红点；点击展开 popover；点重试调 forceProbe；degraded 渲染 muted 角标 |

### 9.1 手测脚本（实施时 verify）

1. `pnpm tauri:dev` 起来，断 WiFi → 30s 内顶栏出现红点。
2. 红点出现期间发消息 → 看到 toast「网络不通，消息可能发送失败」。
3. 恢复 WiFi → 红点消失（≤ 30s）。
4. 模拟 5xx（host 改指向坏端口或 mock）→ 灰色 degraded 角标，发消息**不**弹 toast，由 LLM 失败流处理。
5. 系统休眠 5 分钟唤醒 → 30s 内重新探测，不雪崩、不假阳性。
6. 连点重试按钮 5 次 → 仅触发 1 次 force probe（节流验证）。

## 10. 风险与缓解

| 风险 | 缓解 |
|---|---|
| reqwest 5s 超时与 LLM 长流式请求互相影响 | 独立 `reqwest::Client` 实例，不复用 |
| 状态 flapping 抖动 UI | 失败退避 + 连续 3 次成功才回 30s |
| macOS 休眠唤醒一次性补发 N 次 tick | `MissedTickBehavior::Skip` |
| 前端冷启动几百毫秒红点闪烁 | 初值 `unknown` 时一律不渲染；启动后 `network_get_status` 拿初值 |
| degraded 状态与 LLM 错误 toast 重复打扰 | 发送 toast 仅在 offline 触发；degraded 只挂角标 |
| 探测请求过度暴露内部行为给后端 | HEAD 请求与正常 LLM 请求无差异，无新增端点 |

## 11. 不做的事

- 离线消息队列 / 自动重发。
- 多目标分层探测（lotus 失败再 ping 公共 DNS）。
- 打开系统网络设置按钮。
- warning 主题色变量。
- 替换现有 `sendFailedDesc` / LLM 错误流的文案。
- 把 `errorKind` 暴露给用户 UI。
