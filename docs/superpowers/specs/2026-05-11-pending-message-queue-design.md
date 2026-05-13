# Pending Message Queue — 设计文档

> **状态**：设计稿 · 待用户 review
> **日期**：2026-05-11
> **作者**：Claude（基于与项目 owner 的头脑风暴）
> **关联工件**：
> - 现状代码：`src-tauri/src/connector/channel/manager.rs`、`src-tauri/src/runtime/run_registry.rs`、`src-tauri/src/runtime/agent/task_notification.rs`
> - 多模态依赖：275c worktree 的 `src-tauri/src/runtime/chat/multimodal.rs` + `src-tauri/src/llm/vision_support.rs`（PR P7）
> - 关联 spec：`docs/superpowers/specs/2026-05-06-im-channel-dingtalk-design.md`、`docs/superpowers/specs/2026-05-09-rich-composer-tiptap-design.md`

---

## 1. 背景与问题陈述

钉钉 IM 接入打通后，会话进入"上一轮 LLM 还没回完，新消息已经堆过来"的常态。现状的处理路径是：

1. `connector/channel/manager.rs` worker 收到钉钉消息
2. 持久化为 user message + 调 `ChatAdapter::send_chat_request` (spawn 独立 task)
3. `RuntimeRunRegistry::reserve(session_id)` 发现该 session 已有正在跑的 run → 返回 `"This conversation is already processing."`
4. worker 仅 `log::error!` 后**丢弃**这条消息

这意味着钉钉用户在 LLM 思考期间发的连环消息会全部被吞掉，LLM 永远看不到。

同时，在 app 内普通对话场景，用户在 LLM 流式输出期间想接着补话，目前 composer 只能等流结束、且没有"我接着说几句、稍后一起处理"的体感。

**目标**：一套统一的"忙时排队、空闲合并"的机制，覆盖钉钉 IM 和 app 内 composer 两类入口，pending 状态在 app UI 上可见��可单条移除。

---

## 2. 非目标

- **不**改钉钉 stream 订阅 / token 刷新 / 卡片回复 等 IM 基础设施
- **不**触碰 `ask_coordinator` 对 pending ask 回复的拦截语义（pending ask 优先级**高于**新引入的 pending 队列）
- **不**做跨设备 / 跨用户的 pending 同步
- **不**做"用户编辑 pending 内容"的 UX
- **不**新增任何 OpenAI 端的多模态行为变更，多模态仅按 P7 既有边界（Anthropic 路径、当前 turn 限制 4 张/单张 3 MB/总计 6 MB）

---

## 3. 总体架构

```
┌─────────────────┐      ┌──────────────┐
│ 钉钉 IM worker  │──┐   │ app composer │──┐
└─────────────────┘  │   └──────────────┘  │
                     ▼                     ▼
            ┌──────────────────────────────┐
            │  PendingQueueManager (后端)  │
            │  Mutex<HashMap<SessionId,    │
            │            SessionPending>>  │
            │   ↕ pending.json (per conv)  │
            └──────────────────────────────┘
                     │
       enqueue_or_send(session, item):
         ├─ session 不忙 → 立即 spawn send_chat_request（含本条）
         └─ session 忙   → push 到队列 + 写盘 + 推 pending:queued
                     │
       turn 结束 (StreamDone) → 防抖 1.2s →
         ├─ 队列空 → no-op
         └─ 队列非空 → drain 内存 + 清盘 + 推 pending:drained
                       → 合并成 ChatTurnRequest（多条 user message） → send_chat_request
                     │
                     ▼
              前端 chatStore 监听事件，画 chips
```

**职责分层**

- **后端**：唯一真相源（队列状态 + 持久化 + 防抖定时器 + 合并逻辑 + 事件分发）
- **前端**：被动渲染（订阅事件 → per-session chips；用户 × 删除时调 IPC）
- **持久化**：per-conv 目录下新增 `pending.json` 兄弟文件，**不**污染 `conv.json`

---

## 4. 数据结构

### 4.1 后端类型（`runtime/pending/types.rs`）

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingItem {
    pub id: String,                        // pend-{uuid}
    pub source: PendingSource,
    pub text: String,                      // 原始文本（不含发送者前缀）
    pub sender_nick: Option<String>,       // IM 群聊有；私聊和 app 入口为 None
    pub attachments: Vec<PendingAttachment>,
    pub received_at: String,               // RFC3339
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingAttachment {
    pub id: String,
    pub file_path: String,                 // 绝对路径
    pub mime: Option<String>,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PendingSource {
    App,
    ImDingtalk,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PendingFileFormat {
    pub schema_version: u32,               // 当前 1
    pub items: Vec<PendingItem>,
}

pub enum EnqueueOutcome {
    SentDirectly { request: ChatTurnRequest },
    Queued { snapshot: Vec<PendingItem> },
    Rejected { reason: EnqueueRejection },
}

pub enum EnqueueRejection {
    QueueFull { limit: usize },            // 当前 50
    SessionArchived,
}
```

### 4.2 前端类型（`src/types/pending.ts`）

```ts
export type PendingSource = 'app' | 'im-dingtalk';

export interface PendingAttachment {
  id: string;
  filePath: string;
  mime?: string | null;
  sizeBytes?: number | null;
}

export interface PendingItem {
  id: string;
  source: PendingSource;
  text: string;
  senderNick?: string | null;
  attachments: PendingAttachment[];
  receivedAt: string;
}
```

### 4.3 持久化文件

```
~/.renlijia/users/{scope}/conversations/{id}/
  ├── conv.json              # 不动
  ├── messages.N.jsonl       # 不动
  ├── _current               # 不动
  ├── pending.json           # 新增
  └── ...
```

`pending.json` 内容：

```json
{
  "schemaVersion": 1,
  "items": [
    {
      "id": "pend-7c1f...",
      "source": "im-dingtalk",
      "text": "帮我看下这个表",
      "senderNick": "张三",
      "attachments": [
        {
          "id": "att-...",
          "filePath": "/Users/oayzz/.renlijia/.../uploads/Q1.xlsx",
          "mime": "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
          "sizeBytes": 23984
        }
      ],
      "receivedAt": "2026-05-11T03:21:00.123Z"
    }
  ]
}
```

队列空时**保留**空文件（`{"schemaVersion": 1, "items": []}`），便于跨重启 schema check。

---

## 5. 后端核心：PendingQueueManager

### 5.1 内部状态

```rust
struct SessionPending {
    items: Vec<PendingItem>,
    drain_timer: Option<JoinHandle<()>>,        // tokio::spawn 倒计时
    recently_drained: VecDeque<(String, Instant)>, // (item_id, dispatched_at)，10min TTL
}

pub struct PendingQueueManager {
    inner: Arc<Mutex<HashMap<SessionId, SessionPending>>>,
    conv_dir_resolver: Arc<dyn ConvDirResolver>, // 拿到 ~/.renlijia/.../conversations/{id}/ 路径
    run_registry: Arc<RuntimeRunRegistry>,
    event_bus: Arc<RuntimeEventBus>,
    chat_adapter: Arc<dyn ChatAdapter>,
    config: PendingConfig,
}

#[derive(Clone)]
pub struct PendingConfig {
    pub debounce_window: Duration,         // 默认 1200ms
    pub max_queue_per_session: usize,      // 默认 50
    pub recently_drained_ttl: Duration,    // 默认 10min
}
```

### 5.2 公开 API

```rust
impl PendingQueueManager {
    /// 入队入口（IM worker / app composer / Tauri command 都调它）。
    /// 调用方传"已下载/已转换好"的 PendingItem，本方法不下载附件。
    pub async fn enqueue_or_send(
        &self,
        session_id: SessionId,
        item: PendingItem,
    ) -> Result<EnqueueOutcome>;

    /// turn 结束后由 SessionRuntime 调用，启动防抖 timer
    pub async fn schedule_drain(&self, session_id: SessionId);

    /// 用户在 UI 上 × 单条
    pub async fn remove_item(
        &self,
        session_id: &SessionId,
        item_id: &str,
    ) -> Result<bool>;

    /// 前端进入会话时拉快照
    pub async fn snapshot(&self, session_id: &SessionId) -> Vec<PendingItem>;

    /// 启动时调用：扫所有未归档 conv 的 pending.json → 加载到内存
    pub async fn restore_from_disk(&self) -> Result<()>;
}
```

### 5.3 enqueue_or_send 关键不变量

```text
1. 锁 inner mutex
2. 拿 session 状态 SessionPending（不存在则 new）
3. if items.len() >= max_queue_per_session → Rejected::QueueFull
4. if conv is archived → Rejected::SessionArchived
5. 调 run_registry.is_session_busy(session_id):
   - false → 释放锁；构造 ChatTurnRequest（单条直通）；spawn send_chat_request；返回 SentDirectly
   - true → push item 到 items；释放锁；spawn 写 pending.json（atomic）；推 pending:queued；返回 Queued
```

**关键**：is_busy 检查 + push 必须**同把锁**。锁释放后写盘、写事件、spawn task 都不需要锁。

**lock ordering**：永远是 `pending → run_registry`（run_registry 内部不会回查 pending），杜绝死锁。

### 5.4 schedule_drain 逻辑

```text
1. 锁 inner
2. 取 session 状态：
   - 没有 SessionPending → 释放锁，return（队列从未被使用过）
   - items 空 → 释放锁，return
   - items 非空：
     - 取消旧 drain_timer（如果存在）
     - 启动新 timer：tokio::spawn(async move {
         sleep(debounce_window).await;
         drain_and_dispatch(session_id).await;
       })
     - 把新 timer handle 存回 SessionPending.drain_timer
3. 释放锁
```

**防抖重置点**：
- `enqueue_or_send` 在 busy 路径上 push 到队列后，也调一次 `schedule_drain`（重置 timer，避免"防抖期间又来新消息但 timer 没刷新"）。

### 5.5 drain_and_dispatch 逻辑

```text
1. 锁 inner
2. 二次校验 run_registry.is_session_busy(session_id):
   - true → 释放锁，return（被别的 turn 抢跑，等下个 StreamDone 重新 schedule）
3. 把 SessionPending.items take 出来（mem::replace 成空 Vec），drain_timer 清 None
4. 把 take 出来的 items 的 ids push 到 recently_drained（带时间戳）
5. 释放锁
6. spawn:
   a. 写 pending.json 为空（atomic）
   b. 推 RuntimeEvent::PendingDrained { session_id, drained_ids }
   c. 把 items 持久化到 messages.jsonl —— **多条独立 user message**（见 §6）
   d. 构造 ChatTurnRequest，调 chat_adapter.send_chat_request（多条 user message 形态）
```

写盘失败、事件失败、persist 失败：每一步独立 `log::warn!` 但不回滚（最坏情况下重启时由 `recently_drained` 防止重复 dispatch）。

### 5.6 restore_from_disk 逻辑

```text
- 启动时调用一次（在 SessionRuntime 初始化、subscribe 钉钉 stream 之前）
- 遍历 ~/.renlijia/users/*/conversations/*/
- 跳过 isArchived = true 的
- 读取 pending.json：
  - 不存在 → 跳过
  - schema_version != 1 → 重写为空（向前兼容预留）
  - 解析失败 → log::warn + 重写为空
  - items 非空 → 加载到内存（不主动 dispatch）
- 不调 schedule_drain：等下次正常 enqueue / StreamDone 自然触发
```

**为什么不主动 dispatch**：启动后立即 dispatch 可能在用户还没看到 chips 之前就把队列清了，违反 UX 直觉（"我打开 app 想看看堆了哪些消息"）。

### 5.7 重复防护：recently_drained

```text
drain 时每个 item id 入 VecDeque 带 Instant
restore_from_disk 时（理论上极少发生）：
  - 如果磁盘上有 item id 与 recently_drained 重合 → 跳过该 item + 写盘清掉
  - TTL 到了的旧 entry 自动 pop_front
```

这只针对"drain 写完 messages.jsonl 但还没清 pending.json 时进程崩溃"的极端场景。常规路径下 recently_drained 几乎不会被命中。

---

## 6. 落 messages.jsonl 与 LLM 输入形态

### 6.1 落库形态：多条独立 user message

drain 时把队列里 N 条 PendingItem 各自落成 1 条 `Message { role: "user", ... }`。每条独立带：
- `text` — 含 sender 前缀（群聊：`[张三]: ...`；私聊/app：原文）
- `attachments` — 该条的附件列表

UI 上对应 N 个用户气泡，符合"用户连发了 N 条"的直觉。

### 6.2 LLM 输入形态：C 方案

`ChatTurnRequest` 携带"最后 N 条连续 user message"（drain 出来的 N 条），`chat_turn_driver` 把这 N 条以多条 `Message { role: "user" }` 形态发给 provider。

**Anthropic 路径**（Lotus / Claude direct）：服务端官方文档明确说"Consecutive user or assistant turns in your request will be combined into a single turn."—— 我们直接发 N 条 user message，由服务端合并。无需客户端预合并。

**non-Anthropic 路径**（OpenAI / Qwen / DeepSeek / Volcano / Custom）：行为不保证，可能报错或自动合并不可控。统一在 `chat_turn_driver` 出 provider 之前做**客户端预合并**：连续多条 user message 拼成 1 条文本，附件按顺序合并成一个 Vec。预合并发生在 `provider.send` 之前，不污染 messages.jsonl 历史。

实现位置建议在 `chat_turn_driver` 的 provider 选择分支上加一个 `pre_merge_consecutive_user_messages_if_needed(provider_kind, messages)` helper，逻辑居中、容易测。

### 6.3 多模态预算跨多 user message

现状 `multimodal.rs::build_anthropic_image_blocks(attachments: &[ChatAttachmentRef])` 接受单条 user message 的 attachments。改造为接受"最后 N 条连续 user message 的合并 attachments view"，预算（4 张图、单张 ≤ 3 MB、总计 ≤ 6 MB）按整批跨消息统一计数。

被降级的图片仍以"路径附件"形式留在原所属 user message 的文本里（沿用 `retain_text_fallback_attachments` 现有降级语义）。

由于 P7 多模态实现位于 275c worktree、尚未合并到主干，本 spec 不直接 patch `multimodal.rs`，而是在实施 plan 阶段 cherry-pick 协调，明确 PR 顺序：**P7 必须先合 main，pending 队列再叠加多模态预算改造**。

---

## 7. 入口接入：IM worker 与 app composer

### 7.1 钉钉 IM worker 改造

现状（`connector/channel/manager.rs:494-770`）：

```text
recv 钉钉消息
  → 幂等去重 (seen_msg_ids)
  → 路由 session
  → ask_coordinator.try_handle_reply
  → 下载附件 (download_specs_for_turn)
  → build_channel_chat_request
  → reply_manager.register
  → spawn(send_chat_request)
```

改造后：

```text
recv 钉钉消息
  → 幂等去重
  → 路由 session
  → ask_coordinator.try_handle_reply  (不变；pending ask 优先级高于 pending 队列)
  → 下载附件
  → 构造 PendingItem { source: ImDingtalk, sender_nick, text, attachments }
  → reply_manager.register (不变；钉钉端 card 状态需要尽早建)
  → pending_manager.enqueue_or_send(session_id, item).await:
      - SentDirectly { request } → spawn(send_chat_request(request))
      - Queued → 不做后续操作（等防抖触发）
      - Rejected::QueueFull → 通过 sessionWebhook 回钉钉端"消息堆积过多，请稍后再发"
      - Rejected::SessionArchived → 不可达（IM 不会走到归档 session）
```

**注意**：worker 不再直接 persist user message 到 messages.jsonl —— 由 PendingQueueManager 在 SentDirectly / drain 时统一负责。

**死锁规避**（沿用现状）：worker 不能 await `send_chat_request`（turn 内部可能弹 ask_user 等用户在 IM 端回复）。`enqueue_or_send` 内部如果走 SentDirectly 路径，必须 `tokio::spawn` 异步启动 send_chat_request 而非直接 await。

### 7.2 app composer 改造

现状：`transport/tauri_commands/chat.rs` 的 `send_message` Tauri 命令直接调 `ChatAdapter::send_chat_request`，遇到 busy 返回错误给前端。

改造后：

```rust
#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    payload: SendMessagePayload,
) -> Result<SendMessageOutcome, String> {
    let pending_manager = app.state::<Arc<PendingQueueManager>>();
    let item = PendingItem::from_app_payload(&payload);
    let outcome = pending_manager.enqueue_or_send(payload.session_id.into(), item).await?;
    match outcome {
        EnqueueOutcome::SentDirectly { request } => {
            // PendingQueueManager 已 spawn send_chat_request；前端等 stream 事件
            Ok(SendMessageOutcome::Sent { run_id: request.run_id.into() })
        }
        EnqueueOutcome::Queued { snapshot } => {
            Ok(SendMessageOutcome::Queued { queue_size: snapshot.len() })
        }
        EnqueueOutcome::Rejected { reason } => Err(format!("queue rejected: {:?}", reason)),
    }
}
```

`SendMessageOutcome` 是新枚举，前端按 `kind` discriminator 分别处理。

### 7.3 ask_coordinator 优先级

`ask_coordinator.try_handle_reply` 的语义不变。改造后的入口流程**先**问 ask_coordinator，**后**进 pending 队列。理由：pending ask（"请回答这个问题…"）的回复必须 resolve 那个 ask，不能进队列被合并。

如果 ask_coordinator 返回 `Reroute` 或 `NotPending`，再走 pending 路径。这跟现状 `Reroute` 已经走 `send_chat_request` 重新发 turn 的语义一致——只是把那次直接 send 替换成 enqueue_or_send。

---

## 8. 事件协议

### 8.1 RuntimeEvent → 前端 Tauri 事件

新增 4 个 `RuntimeEventKind` 变体，由 `transport/tauri_event_adapter.rs` 映射到前端：

| RuntimeEventKind | 前端 Tauri Event | Payload (camelCase) |
|---|---|---|
| `PendingSnapshot` | `pending:snapshot` | `{ sessionId, items: PendingItem[] }` |
| `PendingQueued` | `pending:queued` | `{ sessionId, item: PendingItem }` |
| `PendingDrained` | `pending:drained` | `{ sessionId, drainedIds: string[] }` |
| `PendingRemoved` | `pending:removed` | `{ sessionId, itemId: string }` |

事件常量加到 `src/lib/tauri.ts` 的 `TAURI_EVENTS` 对象，前端通过常量订阅，不写字符串字面量。

### 8.2 前端 → 后端 Tauri 命令

新增 2 个：

```rust
#[tauri::command]
pub async fn pending_snapshot_for_session(
    app: AppHandle,
    session_id: String,
) -> Result<Vec<PendingItem>, String>;

#[tauri::command]
pub async fn pending_remove_item(
    app: AppHandle,
    session_id: String,
    item_id: String,
) -> Result<(), String>;
```

`pending_snapshot_for_session` 拉同步快照（不通过事件）；`pending_remove_item` 调 manager.remove_item，manager 内部 push `PendingRemoved` 事件给所有 listener。

---

## 9. 前端实现

### 9.1 store (`src/stores/pendingStore.ts`)

```ts
export interface PendingState {
  bySession: Record<string, PendingItem[]>;
  applySnapshot: (sessionId: string, items: PendingItem[]) => void;
  applyQueued: (sessionId: string, item: PendingItem) => void;
  applyDrained: (sessionId: string, drainedIds: string[]) => void;
  applyRemoved: (sessionId: string, itemId: string) => void;
  removeItem: (sessionId: string, itemId: string) => Promise<void>;
}
```

`applyDrained` reducer：从 bySession[sessionId] 里过滤掉 drainedIds（防止后端事件丢失时本地残留）。

`removeItem` action：调 IPC `pendingRemoveItem(sessionId, itemId)`；不在本地 reducer 直接 splice —— 等后端 `pending:removed` 事件回来再删，单一真相源。

### 9.2 事件订阅 (`src/App.tsx`)

参考现有 `useDragDropListener` 模式，App 顶层 mount 一次：

```tsx
useTauriEvent(TAURI_EVENTS.PENDING_SNAPSHOT, (e) => {
  pendingStore.applySnapshot(e.payload.sessionId, e.payload.items);
});
useTauriEvent(TAURI_EVENTS.PENDING_QUEUED, ...);
useTauriEvent(TAURI_EVENTS.PENDING_DRAINED, ...);
useTauriEvent(TAURI_EVENTS.PENDING_REMOVED, ...);
```

ChatPage mount 时调一次 `pendingSnapshotForSession(sessionId)` → `applySnapshot`，保证切换会话立即可见。

### 9.3 UI 组件 (`src/features/chat/PendingChips.tsx`)

```tsx
export function PendingChips({ sessionId }: { sessionId: string }) {
  const items = usePendingStore((s) => s.bySession[sessionId] ?? []);
  const removeItem = usePendingStore((s) => s.removeItem);

  if (items.length === 0) return null;

  return (
    <div className="flex flex-wrap gap-1.5 px-3 py-2 border-t border-border bg-muted/30">
      <span className="text-xs text-muted-foreground self-center">
        {items.length === 1
          ? t('chat.pending.singleHint')
          : t('chat.pending.batchHint', { count: items.length })}
      </span>
      {items.map((item) => (
        <PendingChip
          key={item.id}
          item={item}
          onRemove={() => removeItem(sessionId, item.id)}
        />
      ))}
    </div>
  );
}
```

`PendingChip` 单条样式（颜色全部走主题变量，符合 `CLAUDE.md` 强约束）：

```tsx
<div className="
  inline-flex items-center gap-1.5 max-w-xs
  px-2 py-1 rounded-md
  bg-muted text-muted-foreground text-xs
  border border-border
">
  <SourceIcon source={item.source} className="w-3.5 h-3.5 shrink-0" />
  {item.senderNick && (
    <span className="font-medium text-foreground shrink-0">{item.senderNick}:</span>
  )}
  <span className="truncate">{truncate(item.text, 30)}</span>
  {item.attachments.length > 0 && <Paperclip className="w-3 h-3 shrink-0" />}
  <button
    type="button"
    onClick={onRemove}
    aria-label={t('chat.pending.removeAria')}
    className="ml-0.5 shrink-0 hover:bg-destructive/10 hover:text-destructive rounded p-0.5 transition-colors"
  >
    <X className="w-3 h-3" />
  </button>
</div>
```

### 9.4 集成位置

`ChatBottomArea`（app 自用） 和钉钉会话占位 composer 区域统一插入 `<PendingChips sessionId={current} />`。两类会话**复用同一组件、同一 store**，无视觉差异。

---

## 10. 边界场景

| 场景 | 行为 |
|---|---|
| 用户在 LLM 流式输出期间点"中止" | 队列**保留**；中止后立即 `schedule_drain`（防抖窗口正常计） |
| 用户清空对话 | `delete_conversation` 顺带删 pending.json + 内存 entry |
| 会话归档 | `archive_conversation` 后 `enqueue_or_send` 返回 `Rejected::SessionArchived`；IM 入口降级为 sessionWebhook 提示 |
| pending.json 损坏 | 当作空队列 + log warn + 重写空文件 |
| 队列长度 > 50 | `Rejected::QueueFull`；IM 入口走 sessionWebhook 回提示，app 入口前端 toast |
| 防抖期间 session 被另一个 turn 抢跑（如 ask_coordinator reroute） | drain 二次校验 is_busy → 跳过；下一次 StreamDone 重新 schedule |
| drain 写 messages.jsonl 失败 | log warn；item 已经从 pending.json 清掉，最坏情况是 LLM 没看到这批（极端） |
| drain 触发 send_chat_request 但 LLM 网关 4xx | LLM 错误正常上报 stream:error；item 仍已落 messages.jsonl，下次用户输入时作为 history 自然带回 |
| 进程在 drain dispatch 中崩溃 | 重启 restore_from_disk 时通过 recently_drained set 防重复 |
| 多窗口/多 tab 同时打开同一会话 | 后端事件 broadcast 给所有 listener；store 单例（zustand），UI 一致 |
| 用户在 chips 渲染期间点 × 同时 LLM 完成 → drain | manager 持锁串行：要么 drain 先（item 已不在队列，× 是 noop）要么 remove 先（drain 时少一条），都正确 |

---

## 11. 实施约束与依赖

### 11.1 依赖工件

1. **P7 多模态先合 main**：本 spec 6.3 节涉及 `multimodal.rs::build_anthropic_image_blocks` 的预算改造。P7 还在 275c worktree，必须先合并到 main 才能开工。
2. **`RuntimeRunRegistry::is_session_busy`** 已存在（`run_registry.rs:132`），无需新增 API。
3. **`AiJiaHome::user_conversations_dir`** 已存在，新增 `pending_json_path(session_id)` helper。

### 11.2 模块边界

- 新增 `runtime/pending/` 不得 `use tauri::*`（沿用 runtime 层不依赖 Tauri 的硬约束）。事件分发通过 `RuntimeEventBus` trait。
- 新增 Tauri 命令位于 `transport/tauri_commands/pending.rs`，是 transport 适配层，**只**做参数解包 + 调 PendingQueueManager。
- 持久化用 `storage::file_store::atomic_write_json`，不直接 fs::write。

### 11.3 性能考量

- 锁内禁止 IO：所有 `std::fs::*` / `tokio::fs::*` 调用必须在锁释放后做
- 写 pending.json 频率：单 session 高峰估计 5–10 次/分钟，atomic_write_json 在 macOS / Windows / Linux 都是 ms 级，无瓶颈
- 启动 restore_from_disk：扫所有 conv 目录，假设单用户 ≤ 1000 个 conv，预期 < 200ms（顺序读小文件）。如果未来超过这个量级，加并行（rayon）

---

## 12. 测试计划

### 12.1 后端单测（`runtime/pending/tests/`）

- `enqueue_busy_path_queues`：is_busy=true → Queued
- `enqueue_idle_path_sends`：is_busy=false → SentDirectly
- `enqueue_archived_session_rejected`
- `enqueue_full_queue_rejected`
- `enqueue_idempotent_within_lock`：并发 100 个 enqueue，verify 无 lost-update
- `schedule_drain_resets_timer_on_new_enqueue`
- `drain_skipped_when_session_becomes_busy`
- `drain_persists_messages_to_jsonl`
- `drain_writes_pending_json_empty_after`
- `remove_item_removes_from_disk_and_memory`
- `restore_from_disk_loads_unarchived_only`
- `restore_skips_recently_drained_ids`
- `pending_json_corrupted_falls_back_to_empty`

### 12.2 后端集成测试（`src-tauri/tests/`）

- `pending_im_app_unified_test.rs`：构造假 IM worker + 假 app composer 并发 enqueue → drain 后顺序 = received_at 升序
- `pending_drain_during_busy_test.rs`：drain timer 到期时插入一个 reserve → drain 跳过 → 下个 StreamDone 重新 schedule
- `pending_review_no_tauri_dep.rs`：架构约束（grep `runtime/pending` 不含 `use tauri`）
- `pending_review_messages_jsonl_landed.rs`：drain 后 N 条 user message 顺序、attachments 完整

### 12.3 前端单测（`vitest`）

- `pendingStore.test.ts`：snapshot/queued/drained/removed 4 个 reducer 正确性
- `PendingChips.test.tsx`：0 条不渲染、1 条渲染单条、N 条渲染多条 + 头部计数
- `PendingChip.test.tsx`：sender 前缀、附件 icon、× 调用 onRemove

### 12.4 前端集成测试

- `chatStore.integration.test.tsx`：mock 后端事件 → drain 后 chatStore 增加 N 条 user message + chips 消失

### 12.5 多模态回归

- `multimodal_test.rs`：N 条 user message 跨消息预算、超出预算的图正确降级到所属 user message 的文本

### 12.6 non-Anthropic 预合并

- `chat_turn_driver_test.rs`：OpenAI/Qwen/DeepSeek provider 收到的最终请求中，连续 user message 已被合并

---

## 13. 迁移与发布

### 13.1 灰度

无配置开关，直接全量上线。原因：
- 现状是"消息丢失"的 bug，不存在"老路径稳定不能动"的反向风险
- 队列默认行为退化等价于"立即发"（队列空时 schedule_drain 是 noop；不忙时 enqueue_or_send 走 SentDirectly）

### 13.2 schema 演进

`pending.json::schemaVersion = 1`。未来字段新增走 `serde(default)` 兼容；不兼容变更走 `schemaVersion += 1` + 启动时迁移函数。

### 13.3 发版顺序

1. P7 多模态合并到 main（前置）
2. 本 spec 实施 PR：拆 5 个独立 PR
   - PR1：`runtime/pending/` 骨架 + 单测（不接入入口）
   - PR2：`transport/tauri_event_adapter` + `tauri_commands/pending` + 前端 store 和 UI
   - PR3：钉钉 IM worker 接入 enqueue_or_send
   - PR4：app composer 接入 enqueue_or_send
   - PR5：multimodal 跨多 user message 预算 + non-Anthropic 预合并

每个 PR 内部使用 TDD（`superpowers:test-driven-development`）。

### 13.4 监控与回滚

- 加 log span：每次 enqueue / drain 打 INFO 级日志（session_id、queue size、source）
- 关键 log 事件：`[pending] enqueue queued`、`[pending] drain dispatched n=X`、`[pending] queue rejected reason=Y`
- 回滚：单 PR 回滚即可，pending.json 文件保留无害（restore 时空读）

---

## 14. 未决问题（不阻塞落 spec）

1. 队列上限 50 是否合理？需要在灰度后看真实数据调整。
2. 防抖窗口 1.2s 是否最优？候选范围 800ms–2s，需要用户体验调优。
3. 钉钉群里**多个不同发送者**同时发消息进入 pending，drain 时要不要按发送者分组合并？目前不分组（按时间序合并），第一版上线后看反馈。
4. 未来是否要扩展到非聊天场景（schedules / employee dispatch）？暂不考虑。

---

## 15. 决策附录

| 议题 | 选项 | 决策 | 理由 |
|---|---|---|---|
| 触发模型 | A 仅 turn 末 / B 仅防抖 / **A+B** | A+B | A+B 兼顾稳定性和"连环消息"友好 |
| 真相源 | 前端 / **后端** / 双套 | 后端 | 单一真相源，IM/app 统一 |
| 持久化载体 | conv.json 内 / **pending.json 旁文件** / 不持久化 | pending.json | 冷热数据分离，避免 conv.json 热点 |
| chips 位置 | composer 上方 / 消息列表内 / 都做 | **composer 上方** | 用户偏好 |
| 取消 UX | 整体清空 / **per-item ×** | per-item × | 更克制、误操作代价小 |
| messages.jsonl 形态 | 1 条合并 / **N 条独立** | N 条独立 | UI 自然，重放语义清晰 |
| LLM 输入形态 | 客户端合并 / **多条 user message** | C 方案 | Anthropic 服务端自动合并；non-Anthropic 客户端预合并 |
| pending 落库时机 | enqueue 时 / **drain 时** | drain 时 | × 删除后历史干净，不留孤儿消息 |
