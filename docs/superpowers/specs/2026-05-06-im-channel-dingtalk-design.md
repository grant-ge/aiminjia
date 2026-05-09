# IM 频道功能设计：钉钉 Stream 接入

**日期**：2026-05-06  
**范围**：钉钉机器人 Stream 模式接入，支持群聊共享会话 + 私聊独立会话，前端频道面板

---

## 1. 目标与约束

### 目标
- 用户在钉钉群里 @机器人 或私聊机器人，触发 AIjia 本地 AI 处理，回复发回钉钉
- AIjia App 内有「频道」面板，展示所有钉钉会话，可看到发送者信息
- 架构为飞书等后续 IM 接入预留扩展点

### 约束
- **方案 A 在线模式**：仅 App 运行时机器人在线，App 关闭时断开连接
- **用户自建应用**：用户在钉钉开发者后台创建企业内部应用，填入 AppKey/AppSecret/RobotCode
- **凭证用户隔离**：同一台设备多账号登录时，频道配置互不干扰

---

## 2. 整体架构

新增 **Channel 层**，位于 `connector/` 内，通过 `ChannelManager` 统一管理：

```
前端（频道面板）
  ↕ Tauri IPC (channel:* 命令 + 事件)
ChannelManager                     ← 新增，src-tauri/src/connector/channel/
  ├── DingtalkStreamClient         ← 新增，WebSocket 长连接
  │     ↕ wss:// (钉钉 Stream 协议)
  │   钉钉服务器
  └── （未来）LarkStreamClient
  ↓ 收到消息
ChannelSessionRouter               ← 新增，群/私聊 → SessionId 映射
  ↓
SessionRuntime                     ← 现有，LLM 对话处理
  ↓ RuntimeEvent
TauriEventAdapter                  ← 现有，推送给前端
  ↓
DingtalkStreamClient.send_reply()  ← 回复发回钉钉（复用现有 dws send-by-bot）
```

### 连接生命周期

1. App 启动，用户已登录 → `ChannelManager` 读取当前用户的频道配置
2. 若配置存在且有效 → 自动建立 Stream 连接
3. App 关闭 → 优雅关闭 WebSocket，不丢失进行中的回复
4. 连接状态变化 → 发 `channel:status` 事件给前端

---

## 3. Session 路由

`ChannelSessionRouter` 维护两张映射表，持久化到用户频道目录：

```
群聊：(platform="dingtalk", conversation_id="openConversationId") → 共享 SessionId
私聊：(platform="dingtalk", user_id="senderUserId")              → 独立 SessionId
```

### 路由逻辑（收到钉钉 Stream 事件时）

1. 解析事件：判断群聊（含 `conversationId`）还是私聊
2. 查映射表 → 找到或新建对应 SessionId
3. 构造 `ChannelMessage`（含 `sender_id`、`sender_nick`）
4. 注入 `SessionRuntime::run_chat_request()`
5. AI 回复通过 `DingtalkStreamClient::send_reply()` 发回钉钉

### 消息 metadata

进入 SessionRuntime 的消息携带 `channel_source` 字段：

```json
{
  "channel_source": {
    "platform": "dingtalk",
    "conversation_type": "group",
    "conversation_id": "cidXXX",
    "sender_id": "userXXX",
    "sender_nick": "张三",
    "msg_id": "msgXXX"
  }
}
```

`msg_id` 用于幂等去重，防止重复投递。

---

## 4. 存储路径

遵循现有 `UserScopedPaths` 模式，在 `user_scoped_paths.rs` 新增：

```
~/.renlijia/users/t_{tenant_id}__u_{user_id}/
  channels/
    dingtalk_config.json    ← AppKey / RobotCode（AppSecret 走现有加密存储）
    dingtalk_sessions.json  ← 群/私聊 → SessionId 映射表
```

`UserScopedPaths` 新增两个方法：

```rust
pub fn channels_dir(&self) -> PathBuf {
    self.base.join("channels")
}
pub fn channel_config_path(&self, platform: &str) -> PathBuf {
    self.base.join("channels").join(format!("{}_config.json", platform))
}
pub fn channel_sessions_path(&self, platform: &str) -> PathBuf {
    self.base.join("channels").join(format!("{}_sessions.json", platform))
}
```

频道会话本身存入现有 `conversations/` 目录（已用户隔离），`channel_source` 字段标记来源。

---

## 5. 前端 UI

### 5.1 频道面板（左侧导航新增 tab）

```
┌─────────────────────────────┐
│  频道                    ＋ │
├─────────────────────────────┤
│  ● 钉钉                     │  ← ● 绿=已连接 ◌ 灰=重连中 ✕ 红=配置有误
│    私聊                     │
│      张三              3    │  ← 未读数
│      李四                   │
│    群聊                     │
│      产品组            1    │
│      技术群                 │
├─────────────────────────────┤
│  ○ 飞书（未配置）           │
└─────────────────────────────┘
```

### 5.2 聊天视图（复用现有聊天 UI）

差异点：
- 群聊消息气泡左上角显示发送者昵称 + 头像首字母
- 顶部 banner 显示「钉钉 · 产品组」，区分来源
- AI 回复旁有小标识「已回复到钉钉」

### 5.3 配置入口（首次点击「+ 添加频道 → 钉钉」）

```
AppKey      [________________]
AppSecret   [________________]  （输入后加密存储，不明文展示）
RobotCode   [________________]
            [连接测试]  [保存]
```

配置保存后立即尝试建立 Stream 连接并验证。

---

## 6. 错误处理与连接状态

### 6.1 状态机

```
未配置 → 已配置/断开 → 连接中 → 已连接
              ↑              ↓
           重连等待 ← 连接断开/出错
```

### 6.2 重连策略

指数退避：1s → 2s → 4s → ... 最大 60s，无限重试直到 App 关闭。  
网络恢复后自动重连，无需用户操作。

### 6.3 关键错误场景

| 场景 | 处理方式 |
|------|---------|
| AppKey/AppSecret 有误 | 钉钉返回 401，停止重连，前端显示「配置有误，请检查」 |
| 网络断开 | 触发重连，前端显示「重连中(12s)...」 |
| AI 处理超时/出错 | 回复钉钉「处理失败，请稍后重试」，不影响连接状态 |
| 消息重复投递 | 用 `msg_id` 幂等去重，丢弃重复 |
| App 关闭 | 优雅关闭 WebSocket，不丢失进行中的 AI 回复 |

### 6.4 前端状态展示

```
● 已连接
◌ 重连中(12s)
✕ 配置有误（点击跳转配置页）
○ 未配置
```

---

## 7. 新增文件清单

### Rust

```
src-tauri/src/connector/channel/
  mod.rs               ← Channel 模块入口
  manager.rs           ← ChannelManager（管理多平台连接生命周期）
  router.rs            ← ChannelSessionRouter（消息路由 + 映射持久化）
  dingtalk_stream.rs   ← DingtalkStreamClient（Stream WebSocket 实现）
  types.rs             ← ChannelMessage、ChannelConfig、ChannelStatus 等类型
src-tauri/src/commands/channel.rs  ← Tauri commands：connect/disconnect/get_status/save_config
```

### 前端

```
src/features/channel/
  ChannelPanel.tsx     ← 左侧频道列表
  ChannelChat.tsx      ← 频道聊天视图（复用 ChatView，注入 sender 信息）
  ChannelConfig.tsx    ← 平台配置表单
  useChannelStatus.ts  ← 订阅 channel:status 事件
src/lib/tauri.ts       ← 新增 channel:* IPC 封装（现有文件扩展）
```

---

## 8. 不在本次范围内

- App 关闭后机器人保持在线（后台 service 模式）
- 飞书接入（架构已预留，下一期）
- `conversations/` 全量用户隔离迁移（独立历史债，不在本次范围）
- 钉钉 ISV 统一应用（OAuth 授权模式，下一期）
