# switchConversation: overwrite → merge-by-id

**Date**: 2026-05-19
**Scope**: `src/hooks/useChat.ts` `switchConversation`
**Symptom (pre-fix)**: 从 home composer 发送首条消息,user 气泡有时会消失（race 频率最近变高）。

## 根因

home → chat 路径下,两个时序事件并发:

1. `HomeTaskComposerCard.handleSubmit`
   - `createConversation()` 拿到 backendId
   - `setRoute({ kind: 'chat', conversationId: backendId })`
   - `sendUserMessage(...)` 走 `useChat.addMessage` 注入 optimistic user bubble

2. `ChatPage` mount 后 `useEffect` → `switchConversation(conversationId)`
   - 进入时 `setMessages([])` 清空
   - `getMessages(id)` 返回后端列表,`setMessages(msgs)` 覆盖

如果 ChatPage 的 `getMessages` 在后端 T9（user msg 落盘）之前完成,返回的列表里没有该 user msg。`setMessages(msgs)` 把刚注入的 optimistic bubble 覆盖掉。后续后端 echo 通过 `useStreaming.ts:411` 的 `clientMessageId` 反查 optimistic 时已经找不到,fall back 成"插入新消息",顺序/视觉错位。

之前讨论过的 3 个候选方案:

- **F1** 前端 hint flag（`suppressNextSwitchFor`）— 小但只 patch home 一条入口
- **F2** `switchConversation` 改 merge-by-id — 架构层修正,所有调用点受益 ✅
- **F3** 后端 `pre_persisted` flag 扩展到普通 send_message — 物理消除 race 但跨前后端

选 F2。

## 改动

`switchConversation` 两处:

1. 初始 `setMessages([])` → `setMessages(filter(m => m.conversationId === id))`
   - 切回当前会话时保留 optimistic 气泡;切到其他会话时仍然清掉跨会话残留

2. `getMessages` 返回后 `setMessages(msgs)` 覆盖 → merge-by-id:
   - fetched 列表为权威
   - 保留 store 中:`conversationId === id` 且 `id` 不在 fetched ids 也不在 fetched 任何 `clientMessageId` 中的消息
   - 按 `createdAt` 合并排序

## 为什么 race 物理消失

- 后端 T9 前 `getMessages` 不含 user msg
- store 里的 optimistic bubble（id === clientMessageId）既不在 fetched ids 也未被 echo 关联 → 落入 storeOnly,合并保留
- 后端 echo 到达时 optimistic 还在 store,`useStreaming.ts:411` 正常 by-id 替换

## 收益

任何"新建即发送"路径（专家团首条、agenda 创建等）沿用 `switchConversation` 都自动受益,不再需要每个入口加 hint flag。

## 测试

- `src/stores/chatStore.test.ts` + `src/hooks/useStreaming.integration.test.tsx` 53/53 通过
- `src/hooks/useChat.test.ts` 的 2 个失败是 pre-existing（sendMessage IPC 签名变化）,与本改动无关
