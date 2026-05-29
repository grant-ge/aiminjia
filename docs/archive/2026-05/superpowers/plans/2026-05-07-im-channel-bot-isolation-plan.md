# IM 频道机器人隔离 + Hydrate 修复 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让钉钉对话按 `robot_code` 隔离（切换式），换机器人后老对话折叠为只读历史；同时修复重启后必须先进 ChannelPage 才能看到对话的 bug。

**Architecture:** `sessions.json` schema 升 v2，key 加 `robot_code` 维度。`ChannelManager` 启动时调 `hydrate_conversations` 从 router + conversation_store 重建内存对话表。检测到 v1 sessions.json 时自动清掉孤儿对话目录（未上线，可丢）。`ChannelConversation` 加 `robot_code` / `is_active_robot`，前端按 `is_active_robot` 拆活跃 / 折叠两组。

**Tech Stack:** Rust 1.x（tokio, serde, anyhow, tempfile）/ React 18 + TypeScript / zustand / Vitest / Tauri 2.x

**Spec:** `docs/superpowers/specs/2026-05-07-im-channel-bot-isolation-design.md`

---

## 文件结构（新增 / 修改一览）

**Rust（后端）**

- 修改：`src-tauri/src/connector/channel/router.rs` —— SessionsState 加 `schema_version`，key 加 `robot_code` 维度，新增 `migrate_or_load` / `entries`
- 修改：`src-tauri/src/connector/channel/types.rs` —— `ChannelConversation` 加 `robot_code` / `is_active_robot`
- 修改：`src-tauri/src/connector/channel/manager.rs` —— 加 `hydrate_conversations` / `refresh_active_robot_flags`，stream worker 改 router 调用，`remove_platform` 不 clear
- 修改：`src-tauri/src/lib.rs` —— `ChannelManager::new` 之后调一次 `hydrate_conversations`
- 新增：`src-tauri/tests/channel_hydrate_test.rs` —— hydrate / 迁移 / refresh 集成测试

**TypeScript（前端）**

- 修改：`src/lib/tauri.ts` —— `ChannelConversation` 类型加字段
- 修改：`src/stores/channelStore.ts` —— `initChannelListeners` 拉对话；`onChannelPlatformState` 回调里也拉一次
- 修改：`src/components/sidebar/AppSidebar.tsx` —— 折叠区
- 新增：`src/components/sidebar/AppSidebar.test.tsx` —— 折叠区分支测试
- 修改：`src/features/channel/ChannelPage.tsx` —— inactive session 输入区 disabled + banner
- 修改：`src/features/channel/ChannelPage.test.tsx` —— 扩展测试
- 修改：`src/stores/channelStore.test.ts` —— 扩展测试

---

## Task 1：router.rs 加 schema_version + migrate_or_load 框架

**Files:**
- Modify: `src-tauri/src/connector/channel/router.rs`

- [ ] **Step 1: 写失败测试 — 检测 v1 schema 触发 wipe**

把以下测试加到 `router.rs` 现有 `mod tests` 末尾：

```rust
    #[test]
    fn migrate_or_load_drops_legacy_v1_data() {
        use crate::runtime::store::{ConversationStore, InMemoryConversationStore};
        use std::sync::Arc;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("channels/dingtalk/sessions.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        // 写一份 v1（无 schemaVersion 字段）
        let v1 = serde_json::json!({
            "sessions": {
                "group:cid-x": "sess-old-1",
                "private:user-y": "sess-old-2",
            }
        });
        std::fs::write(&path, serde_json::to_string_pretty(&v1).unwrap()).unwrap();

        let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());
        conv_store.create_conversation("sess-old-1", "old1").unwrap();
        conv_store.create_conversation("sess-old-2", "old2").unwrap();

        let router = ChannelSessionRouter::migrate_or_load(&path, conv_store.as_ref()).unwrap();

        // 旧对话已删
        assert!(conv_store.list_conversation_ids().unwrap().is_empty());
        // sessions.json 重写为 v2 空文件
        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["schemaVersion"], 2);
        assert!(parsed["sessions"].as_object().unwrap().is_empty());
        // entries 为空
        assert!(router.entries().is_empty());
    }
```

- [ ] **Step 2: 跑测试，确认失败**

```bash
cd src-tauri && cargo test --test-threads=1 -p app_lib --lib channel::router::tests::migrate_or_load_drops_legacy_v1_data 2>&1 | tail -20
```

预期失败原因：`migrate_or_load` 和 `entries` 不存在，编译都过不了。

- [ ] **Step 3: 改 SessionsState 加 schema_version + 实现 migrate_or_load + entries**

把 `router.rs` 顶部 (line 1-66) 整体替换为：

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::runtime::store::ConversationStore;

use super::types::ConversationType;

const CURRENT_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Default, Serialize, Deserialize)]
struct SessionsState {
    #[serde(default, rename = "schemaVersion")]
    schema_version: u32,
    /// key: "group:{robot_code}:{external_id}" 或 "private:{robot_code}:{external_id}"
    #[serde(default)]
    sessions: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct RouterEntry {
    pub conversation_type: ConversationType,
    pub robot_code: String,
    pub external_id: String,
    pub session_id: String,
}

pub struct ChannelSessionRouter {
    sessions_path: PathBuf,
    state: SessionsState,
}

impl ChannelSessionRouter {
    /// 启动入口：检测到 v1（缺失 schema_version 或 != 2）时清空所有指向的 conversation 目录
    /// 后写入空 v2 文件，返回空 router。已是 v2 则正常加载。
    pub fn migrate_or_load(
        sessions_path: &Path,
        conversation_store: &dyn ConversationStore,
    ) -> Result<Self> {
        let state = if sessions_path.exists() {
            let content = std::fs::read_to_string(sessions_path)?;
            serde_json::from_str::<SessionsState>(&content).unwrap_or_default()
        } else {
            SessionsState::default()
        };

        if state.schema_version == CURRENT_SCHEMA_VERSION {
            return Ok(Self {
                sessions_path: sessions_path.to_path_buf(),
                state,
            });
        }

        let legacy_count = state.sessions.len();
        if legacy_count > 0 {
            log::info!(
                "[channel] migrating sessions.json v{} → v{}, dropping {} legacy conversations",
                state.schema_version,
                CURRENT_SCHEMA_VERSION,
                legacy_count
            );
        }
        for session_id in state.sessions.values() {
            if let Err(e) = conversation_store.delete_conversation(session_id) {
                log::warn!(
                    "[channel] failed to delete legacy conversation {}: {:#}",
                    session_id,
                    e
                );
            }
        }

        let mut router = Self {
            sessions_path: sessions_path.to_path_buf(),
            state: SessionsState {
                schema_version: CURRENT_SCHEMA_VERSION,
                sessions: HashMap::new(),
            },
        };
        router.persist()?;
        Ok(router)
    }

    /// 仅供测试 / 老代码路径使用，新代码请用 `migrate_or_load`。
    #[cfg(test)]
    pub fn load(sessions_path: &Path) -> Result<Self> {
        let state = if sessions_path.exists() {
            let content = std::fs::read_to_string(sessions_path)?;
            serde_json::from_str::<SessionsState>(&content).unwrap_or_default()
        } else {
            SessionsState::default()
        };
        Ok(Self {
            sessions_path: sessions_path.to_path_buf(),
            state,
        })
    }

    /// 查询或新建 session_id。新建时持久化到磁盘。
    pub fn get_or_create_session(
        &mut self,
        conversation_type: &ConversationType,
        robot_code: &str,
        external_id: &str,
        create_session: impl FnOnce() -> Result<String>,
    ) -> Result<String> {
        let key = Self::make_key(conversation_type, robot_code, external_id);
        if let Some(session_id) = self.state.sessions.get(&key) {
            return Ok(session_id.clone());
        }
        let session_id = create_session()?;
        self.state.sessions.insert(key, session_id.clone());
        if self.state.schema_version != CURRENT_SCHEMA_VERSION {
            self.state.schema_version = CURRENT_SCHEMA_VERSION;
        }
        self.persist()?;
        Ok(session_id)
    }

    /// 返回所有现存条目，供 hydrate 用。
    pub fn entries(&self) -> Vec<RouterEntry> {
        self.state
            .sessions
            .iter()
            .filter_map(|(key, session_id)| {
                let parsed = Self::parse_key(key)?;
                Some(RouterEntry {
                    conversation_type: parsed.0,
                    robot_code: parsed.1,
                    external_id: parsed.2,
                    session_id: session_id.clone(),
                })
            })
            .collect()
    }

    fn make_key(
        conversation_type: &ConversationType,
        robot_code: &str,
        external_id: &str,
    ) -> String {
        let prefix = match conversation_type {
            ConversationType::Group => "group",
            ConversationType::Private => "private",
        };
        format!("{}:{}:{}", prefix, robot_code, external_id)
    }

    /// 反向解析：把 `group:{robot_code}:{external_id}` 拆出来。
    /// 只识别 v2 格式（包含 robot_code 段）；v1 格式（缺 robot_code）返回 None，由 entries 过滤。
    fn parse_key(key: &str) -> Option<(ConversationType, String, String)> {
        let (prefix, rest) = key.split_once(':')?;
        let conv_type = match prefix {
            "group" => ConversationType::Group,
            "private" => ConversationType::Private,
            _ => return None,
        };
        let (robot_code, external_id) = rest.split_once(':')?;
        if robot_code.is_empty() || external_id.is_empty() {
            return None;
        }
        Some((conv_type, robot_code.to_string(), external_id.to_string()))
    }

    fn persist(&self) -> Result<()> {
        if let Some(parent) = self.sessions_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&self.state)?;
        std::fs::write(&self.sessions_path, content)?;
        Ok(())
    }
}
```

注意：`schemaVersion` 字段名用 `#[serde(rename = "schemaVersion")]` 是为了兼容已有 v1 文件没有该字段（默认 0）+ 新写出来用 camelCase。

- [ ] **Step 4: 更新现有 4 个测试以适配新 API**

把 `mod tests` 里旧的 4 个测试函数替换为：

```rust
    #[test]
    fn creates_new_session_for_group() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("channels/dingtalk/sessions.json");
        let mut router = ChannelSessionRouter::load(&path).unwrap();

        let session_id = router
            .get_or_create_session(&ConversationType::Group, "robot-1", "cid123", || {
                Ok("sess-abc".to_string())
            })
            .unwrap();

        assert_eq!(session_id, "sess-abc");
    }

    #[test]
    fn returns_existing_session_for_same_key() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("channels/dingtalk/sessions.json");
        let mut router = ChannelSessionRouter::load(&path).unwrap();

        router
            .get_or_create_session(&ConversationType::Group, "robot-1", "cid123", || {
                Ok("sess-abc".to_string())
            })
            .unwrap();

        let mut called = false;
        let session_id = router
            .get_or_create_session(&ConversationType::Group, "robot-1", "cid123", || {
                called = true;
                Ok("sess-xyz".to_string())
            })
            .unwrap();

        assert_eq!(session_id, "sess-abc");
        assert!(!called, "closure should not be called for existing session");
    }

    #[test]
    fn group_and_private_use_different_keys() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("channels/dingtalk/sessions.json");
        let mut router = ChannelSessionRouter::load(&path).unwrap();

        let group_sess = router
            .get_or_create_session(&ConversationType::Group, "robot-1", "id123", || {
                Ok("sess-group".to_string())
            })
            .unwrap();

        let private_sess = router
            .get_or_create_session(&ConversationType::Private, "robot-1", "id123", || {
                Ok("sess-private".to_string())
            })
            .unwrap();

        assert_ne!(group_sess, private_sess);
    }

    #[test]
    fn persists_and_reloads() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("channels/dingtalk/sessions.json");

        {
            let mut router = ChannelSessionRouter::load(&path).unwrap();
            router
                .get_or_create_session(&ConversationType::Private, "robot-1", "user42", || {
                    Ok("sess-persisted".to_string())
                })
                .unwrap();
        }

        let mut router2 = ChannelSessionRouter::load(&path).unwrap();
        let mut called = false;
        let session_id = router2
            .get_or_create_session(&ConversationType::Private, "robot-1", "user42", || {
                called = true;
                Ok("sess-new".to_string())
            })
            .unwrap();

        assert_eq!(session_id, "sess-persisted");
        assert!(!called, "should have loaded from disk, not created new");
    }
```

- [ ] **Step 5: 跑测试**

```bash
cd src-tauri && cargo test --lib channel::router 2>&1 | tail -30
```

预期：5 个测试（4 旧 + 1 新）全部 PASS。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/connector/channel/router.rs
git commit -m "feat(channel/router): sessions.json schema v2 + robot_code 维度

- SessionsState 加 schema_version
- get_or_create_session / make_key 加 robot_code 参数
- 新增 migrate_or_load：v1 → v2 时清空指向的 conversation 目录
- 新增 entries() 给 hydrate 用"
```

---

## Task 2：router 补 v2 保留 / key 隔离 / entries 解析 三个测试

**Files:**
- Modify: `src-tauri/src/connector/channel/router.rs`

- [ ] **Step 1: 加三个新测试**

把以下三个测试附加到 `mod tests` 末尾：

```rust
    #[test]
    fn migrate_or_load_preserves_v2_data() {
        use crate::runtime::store::{ConversationStore, InMemoryConversationStore};
        use std::sync::Arc;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("channels/dingtalk/sessions.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        let v2 = serde_json::json!({
            "schemaVersion": 2,
            "sessions": {
                "group:robot-A:cid1": "sess-1",
                "private:robot-A:user2": "sess-2",
            }
        });
        std::fs::write(&path, serde_json::to_string_pretty(&v2).unwrap()).unwrap();

        let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());
        conv_store.create_conversation("sess-1", "t1").unwrap();
        conv_store.create_conversation("sess-2", "t2").unwrap();

        let router = ChannelSessionRouter::migrate_or_load(&path, conv_store.as_ref()).unwrap();

        assert_eq!(router.entries().len(), 2);
        assert_eq!(conv_store.list_conversation_ids().unwrap().len(), 2);
    }

    #[test]
    fn key_includes_robot_code() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("channels/dingtalk/sessions.json");
        let mut router = ChannelSessionRouter::load(&path).unwrap();

        let s_a = router
            .get_or_create_session(&ConversationType::Group, "robot-A", "cid1", || {
                Ok("sess-A".to_string())
            })
            .unwrap();
        let s_b = router
            .get_or_create_session(&ConversationType::Group, "robot-B", "cid1", || {
                Ok("sess-B".to_string())
            })
            .unwrap();

        assert_ne!(s_a, s_b, "same external_id under different robot_code must produce different sessions");
    }

    #[test]
    fn entries_returns_parsed_tuples() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("channels/dingtalk/sessions.json");
        let mut router = ChannelSessionRouter::load(&path).unwrap();

        router
            .get_or_create_session(&ConversationType::Group, "robot-A", "cid1", || {
                Ok("sess-1".to_string())
            })
            .unwrap();
        router
            .get_or_create_session(&ConversationType::Private, "robot-B", "user2", || {
                Ok("sess-2".to_string())
            })
            .unwrap();

        let mut entries = router.entries();
        entries.sort_by(|a, b| a.session_id.cmp(&b.session_id));

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].session_id, "sess-1");
        assert_eq!(entries[0].robot_code, "robot-A");
        assert_eq!(entries[0].external_id, "cid1");
        assert!(matches!(entries[0].conversation_type, ConversationType::Group));

        assert_eq!(entries[1].session_id, "sess-2");
        assert_eq!(entries[1].robot_code, "robot-B");
        assert_eq!(entries[1].external_id, "user2");
        assert!(matches!(entries[1].conversation_type, ConversationType::Private));
    }
```

- [ ] **Step 2: 跑测试**

```bash
cd src-tauri && cargo test --lib channel::router 2>&1 | tail -20
```

预期：8 个测试全部 PASS。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/connector/channel/router.rs
git commit -m "test(channel/router): v2 保留 / robot_code 隔离 / entries 解析"
```

---

## Task 3：types.rs 给 ChannelConversation 加字段

**Files:**
- Modify: `src-tauri/src/connector/channel/types.rs:194-204`

- [ ] **Step 1: 改结构**

把 `types.rs` 第 194-204 行（`ChannelConversation` 结构）替换为：

```rust
/// Channel conversations are internal Lotus sessions backed by an external IM conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelConversation {
    pub session_id: String,
    pub platform: Platform,
    pub conversation_type: ConversationType,
    pub external_id: String,
    pub display_name: String,
    pub unread_count: u32,
    /// 机器人维度，用来区分不同钉钉应用 / 不同机器人产生的对话。
    pub robot_code: String,
    /// 是否归属当前在线机器人；false 表示历史会话，UI 进折叠区，输入区禁用。
    pub is_active_robot: bool,
}
```

- [ ] **Step 2: 跑编译**

```bash
cd src-tauri && cargo build --lib 2>&1 | tail -30
```

预期：会有一些"missing field"错误，来自 `manager.rs` 旧的 push 逻辑。先记下，下一个 task 修。

- [ ] **Step 3: 提交（暂不修编译错误）**

跳过提交，留到 Task 4 一起。

---

## Task 4：manager.rs 把 stream worker 适配新 router API + push 带新字段

**Files:**
- Modify: `src-tauri/src/connector/channel/manager.rs:355-460`

- [ ] **Step 1: 改 stream worker 内部的 router 调用 + push 字段**

找到 `manager.rs:363-435` 那段（消息处理 loop 开头到 session_id 拿到为止）。把 `let mut router = match ChannelSessionRouter::load(&sessions_path)` 改成 `migrate_or_load`：

```rust
        let conv_store_for_worker = Arc::clone(&conv_store);
        let message_handle = tokio::spawn(async move {
            let mut router = match ChannelSessionRouter::migrate_or_load(
                &sessions_path,
                conv_store_for_worker.as_ref(),
            ) {
                Ok(r) => r,
                Err(e) => {
                    log::error!("[channel] failed to load router: {:#}", e);
                    return;
                }
            };
```

把 `router.get_or_create_session(&conv_type, &conv_key, || { ... })` 调用（第 414 行附近）改为带 `robot_code`：

```rust
                let session_id = match router.get_or_create_session(
                    &conv_type,
                    &reply_robot_code,
                    &conv_key,
                    || {
                        let title = match &conv_type_for_create {
                            ConversationType::Group => format!(
                                "钉钉群 {}",
                                &conv_key_for_create[..conv_key_for_create.len().min(8)]
                            ),
                            ConversationType::Private => {
                                format!("钉钉私聊 {}", &sender_nick_for_create)
                            }
                        };
                        let id = uuid::Uuid::new_v4().to_string();
                        store_ref
                            .create_conversation(&id, &title)
                            .map_err(|e| anyhow::anyhow!(e))?;
                        Ok(id)
                    },
                ) {
                    Ok(id) => id,
                    Err(e) => {
                        log::error!("[channel] session routing failed: {:#}", e);
                        continue;
                    }
                };
```

注：`reply_robot_code` 是 `connect_dingtalk` 在 spawn 之前已经 clone 出来的 `String`（manager.rs:296），现在要把它再 clone 一份移进 worker 闭包。在 worker spawn 之前（`let message_handle = tokio::spawn(async move {` 之前那一段，即 `manager.rs:355-362`）加一行：

```rust
        let reply_robot_code_for_worker = reply_robot_code.clone();
```

然后把 `tokio::spawn(async move {` 闭包前的 capture list 加这个变量。worker 内部用 `&reply_robot_code_for_worker`。

把 push `ChannelConversation` 那段（第 450-457 行）改成：

```rust
                        convs_lock.push(ChannelConversation {
                            session_id: session_id.clone(),
                            platform: Platform::Dingtalk,
                            conversation_type: conv_type.clone(),
                            external_id: conv_key.clone(),
                            display_name,
                            unread_count: 0,
                            robot_code: reply_robot_code_for_worker.clone(),
                            is_active_robot: true,
                        });
```

- [ ] **Step 2: 跑编译**

```bash
cd src-tauri && cargo build --lib 2>&1 | tail -30
```

预期：编译通过（types.rs 的两个新字段、router 的新签名、manager 的新调用都对齐了）。

- [ ] **Step 3: 跑现有 channel 测试**

```bash
cd src-tauri && cargo test --lib channel:: 2>&1 | tail -30
```

预期：全部 PASS（router 单测都改过了；manager 现有测试如果有也得过）。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/connector/channel/types.rs src-tauri/src/connector/channel/manager.rs
git commit -m "feat(channel): ChannelConversation 加 robot_code/is_active_robot

- types.rs 加字段
- stream worker 用 migrate_or_load
- get_or_create_session 调用带当前 robot_code
- push 新对话时填 robot_code 和 is_active_robot=true"
```

---

## Task 5：manager.rs 加 hydrate_conversations 纯函数（build_conversation_snapshot）

**Files:**
- Modify: `src-tauri/src/connector/channel/manager.rs`

- [ ] **Step 1: 写失败测试**

在 `manager.rs` 末尾（第 600 行之后）加 `#[cfg(test)] mod hydrate_tests`：

```rust
#[cfg(test)]
mod hydrate_tests {
    use super::*;
    use crate::connector::channel::router::RouterEntry;
    use crate::runtime::store::{ConversationStore, InMemoryConversationStore};
    use std::sync::Arc;

    #[test]
    fn snapshot_marks_only_current_robot_as_active() {
        let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());
        conv_store.create_conversation("sess-1", "Active Title").unwrap();
        conv_store.create_conversation("sess-2", "Legacy Title").unwrap();

        let entries = vec![
            RouterEntry {
                conversation_type: ConversationType::Private,
                robot_code: "robot-current".into(),
                external_id: "user1".into(),
                session_id: "sess-1".into(),
            },
            RouterEntry {
                conversation_type: ConversationType::Group,
                robot_code: "robot-old".into(),
                external_id: "cid2".into(),
                session_id: "sess-2".into(),
            },
        ];

        let snapshot = build_conversation_snapshot(
            &entries,
            conv_store.as_ref(),
            Some("robot-current"),
        );

        assert_eq!(snapshot.len(), 2);
        let active: Vec<_> = snapshot.iter().filter(|c| c.is_active_robot).collect();
        let inactive: Vec<_> = snapshot.iter().filter(|c| !c.is_active_robot).collect();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].session_id, "sess-1");
        assert_eq!(active[0].display_name, "Active Title");
        assert_eq!(inactive.len(), 1);
        assert_eq!(inactive[0].session_id, "sess-2");
        assert_eq!(inactive[0].robot_code, "robot-old");
    }

    #[test]
    fn snapshot_falls_back_to_placeholder_when_title_missing() {
        let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());

        let entries = vec![RouterEntry {
            conversation_type: ConversationType::Private,
            robot_code: "robot-1".into(),
            external_id: "user1".into(),
            session_id: "sess-orphan".into(),
        }];

        let snapshot = build_conversation_snapshot(
            &entries,
            conv_store.as_ref(),
            Some("robot-1"),
        );

        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].display_name, "未知会话");
    }

    #[test]
    fn snapshot_marks_all_inactive_when_no_current_robot() {
        let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());
        conv_store.create_conversation("sess-1", "Title").unwrap();

        let entries = vec![RouterEntry {
            conversation_type: ConversationType::Private,
            robot_code: "robot-1".into(),
            external_id: "user1".into(),
            session_id: "sess-1".into(),
        }];

        let snapshot = build_conversation_snapshot(&entries, conv_store.as_ref(), None);

        assert_eq!(snapshot.len(), 1);
        assert!(!snapshot[0].is_active_robot);
    }
}
```

- [ ] **Step 2: 跑测试，确认失败**

```bash
cd src-tauri && cargo test --lib channel::manager::hydrate_tests 2>&1 | tail -20
```

预期失败原因：`build_conversation_snapshot` 不存在。

- [ ] **Step 3: 实现 build_conversation_snapshot 纯函数**

在 `manager.rs` 末尾（`#[cfg(test)]` 之前，文件作用域）加：

```rust
fn build_conversation_snapshot(
    entries: &[crate::connector::channel::router::RouterEntry],
    conversation_store: &dyn crate::runtime::store::ConversationStore,
    current_robot_code: Option<&str>,
) -> Vec<ChannelConversation> {
    let titles: std::collections::HashMap<String, String> = match conversation_store
        .get_conversations()
    {
        Ok(values) => values
            .into_iter()
            .filter_map(|v| {
                let id = v.get("id").and_then(|x| x.as_str())?.to_string();
                let title = v
                    .get("title")
                    .and_then(|x| x.as_str())
                    .unwrap_or("未知会话")
                    .to_string();
                Some((id, title))
            })
            .collect(),
        Err(e) => {
            log::warn!("[channel] failed to read conversations during hydrate: {:#}", e);
            std::collections::HashMap::new()
        }
    };

    entries
        .iter()
        .map(|entry| {
            let display_name = titles
                .get(&entry.session_id)
                .cloned()
                .unwrap_or_else(|| {
                    log::warn!(
                        "[channel] hydrate: conversation {} not found in store, using placeholder",
                        entry.session_id
                    );
                    "未知会话".to_string()
                });
            let is_active_robot =
                current_robot_code.map(|rc| rc == entry.robot_code).unwrap_or(false);
            ChannelConversation {
                session_id: entry.session_id.clone(),
                platform: Platform::Dingtalk,
                conversation_type: entry.conversation_type.clone(),
                external_id: entry.external_id.clone(),
                display_name,
                unread_count: 0,
                robot_code: entry.robot_code.clone(),
                is_active_robot,
            }
        })
        .collect()
}
```

- [ ] **Step 4: 跑测试**

```bash
cd src-tauri && cargo test --lib channel::manager::hydrate_tests 2>&1 | tail -20
```

预期：3 个测试全 PASS。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/connector/channel/manager.rs
git commit -m "feat(channel/manager): build_conversation_snapshot 纯函数

把 router entries + conversation_store 映射到 ChannelConversation 列表，
按 current_robot_code 设置 is_active_robot。"
```

---

## Task 6：manager.rs 加 hydrate_conversations + refresh_active_robot_flags 异步方法

**Files:**
- Modify: `src-tauri/src/connector/channel/manager.rs`

- [ ] **Step 1: 在 `impl ChannelManager` 块里加两个新方法**

在 `pub async fn auto_connect_if_configured` 之前（约 line 110 之前）加：

```rust
    /// 启动时调用一次：从 sessions.json + conversation_store 重建内存 conversations 列表。
    /// 期间检测到 v1 schema 会清掉所有指向的 conversation 目录（参见 router.migrate_or_load）。
    pub async fn hydrate_conversations(&self) {
        let router = match super::router::ChannelSessionRouter::migrate_or_load(
            &self.sessions_path,
            self.conversation_store.as_ref(),
        ) {
            Ok(r) => r,
            Err(e) => {
                log::error!("[channel] hydrate_conversations: failed to load router: {:#}", e);
                return;
            }
        };
        let entries = router.entries();
        let current_robot = match self.config_store.read_dingtalk_config() {
            Ok(Some(cfg)) => Some(cfg.bot.robot_code),
            Ok(None) => None,
            Err(e) => {
                log::warn!(
                    "[channel] hydrate_conversations: failed to read config: {:#}",
                    e
                );
                None
            }
        };
        let snapshot = build_conversation_snapshot(
            &entries,
            self.conversation_store.as_ref(),
            current_robot.as_deref(),
        );
        *self.conversations.write().await = snapshot;
    }

    /// 重新计算每条 conversation 的 is_active_robot：等于 current_robot_code 的为 true。
    /// 调用方需要保证 emit 一次 platform-state 让前端重拉 conversations。
    pub async fn refresh_active_robot_flags(&self, current_robot_code: Option<&str>) {
        let mut convs = self.conversations.write().await;
        for c in convs.iter_mut() {
            c.is_active_robot = current_robot_code
                .map(|rc| rc == c.robot_code)
                .unwrap_or(false);
        }
    }
```

- [ ] **Step 2: 改 set_connection_state — connected 时刷新 flags**

找到 `set_connection_state`（约 line 95-103）改成：

```rust
    async fn set_connection_state(
        &self,
        connection: ChannelConnectionState,
        last_error: Option<String>,
    ) {
        *self.connection.write().await = connection.clone();
        *self.last_error.write().await = last_error;
        if matches!(connection, ChannelConnectionState::Connected) {
            let current_robot = self
                .config_store
                .read_dingtalk_config()
                .ok()
                .flatten()
                .map(|cfg| cfg.bot.robot_code);
            self.refresh_active_robot_flags(current_robot.as_deref()).await;
        }
        self.emit_dingtalk_state().await;
    }
```

- [ ] **Step 3: 改 remove_platform — 不再 clear，调 refresh(None)**

找到 `remove_platform`（约 line 178-190），把里面 `self.clear_runtime_state().await;` 那一行替换成：

```rust
                self.reply_manager.clear().await;
                self.refresh_active_robot_flags(None).await;
```

注：原 `clear_runtime_state` 是私有 helper（约 line 538-542），它做了 `conversations.clear()` + `reply_manager.clear()`，我们要保留 reply_manager.clear 行为，但 conversations 不 clear。修改 `clear_runtime_state` 同步：

找到 `async fn clear_runtime_state` 实现（约 line 538-542），改成：

```rust
    /// 旧入口：保留供 ChannelManager 内部其它代码调用，但语义变更——只清 reply_manager，
    /// 不再 clear conversations。conversations 由 refresh_active_robot_flags 标记 inactive。
    async fn clear_runtime_state(&self) {
        self.reply_manager.clear().await;
    }
```

然后 `remove_platform` 里把上面写的两行回退成调用 helper：

```rust
                self.clear_runtime_state().await;
                self.refresh_active_robot_flags(None).await;
```

（保持原结构对齐，避免散在两处）

- [ ] **Step 4: 跑编译**

```bash
cd src-tauri && cargo build --lib 2>&1 | tail -20
```

预期：编译通过。

- [ ] **Step 5: 跑现有 channel 测试**

```bash
cd src-tauri && cargo test --lib channel:: 2>&1 | tail -30
```

预期：全部 PASS。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/connector/channel/manager.rs
git commit -m "feat(channel/manager): hydrate_conversations + refresh_active_robot_flags

- hydrate_conversations: 启动期从 router + conversation_store 重建内存表
- refresh_active_robot_flags: connect/remove 时按 current_robot_code 重算
- remove_platform 不再 clear conversations，改成全部翻 inactive
- set_connection_state 进入 Connected 时自动 refresh"
```

---

## Task 7：lib.rs 在 ChannelManager 启动时调一次 hydrate_conversations

**Files:**
- Modify: `src-tauri/src/lib.rs:580-601`

- [ ] **Step 1: 改 spawn 那段**

找到 lib.rs 第 596-599 行：

```rust
                let cm = channel_manager.clone();
                tauri::async_runtime::spawn(async move {
                    cm.auto_connect_if_configured().await;
                });
```

改为：

```rust
                let cm = channel_manager.clone();
                tauri::async_runtime::spawn(async move {
                    cm.hydrate_conversations().await;
                    cm.auto_connect_if_configured().await;
                });
```

- [ ] **Step 2: 跑编译**

```bash
cd src-tauri && cargo build --lib 2>&1 | tail -10
```

预期：编译通过。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(channel): 启动时调 hydrate_conversations

修复重启 App 后必须先进 ChannelPage 才能在侧边栏看到对话的 bug。"
```

---

## Task 8：集成测试 channel_hydrate_test.rs

**Files:**
- Create: `src-tauri/tests/channel_hydrate_test.rs`

集成测试聚焦"router + conversation_store 协作"，避开 ChannelManager 对 AppHandle 的依赖（直接测 build_conversation_snapshot + migrate_or_load 组合）。

- [ ] **Step 1: 检查 build_conversation_snapshot 的可见性**

`build_conversation_snapshot` 当前是 module-private 的。集成测试在 `tests/` 目录下需要 `pub` 才能调用。把 `manager.rs` 里的：

```rust
fn build_conversation_snapshot(
```

改为：

```rust
pub(crate) fn build_conversation_snapshot(
```

跑：

```bash
cd src-tauri && cargo build --lib 2>&1 | tail -5
```

预期：通过。

- [ ] **Step 2: 写集成测试**

新建 `src-tauri/tests/channel_hydrate_test.rs`：

```rust
//! 集成测试：router migration + build_conversation_snapshot 协作。
//!
//! 不直接测 ChannelManager（依赖 AppHandle，集成测试里不便构造），
//! 而是测 ChannelManager 内部纯函数 + ChannelSessionRouter 这两个组件
//! 的组合行为，覆盖 spec §测试 中的 hydrate / 迁移 / refresh 场景。

use std::sync::Arc;

use app_lib::connector::channel::manager::build_conversation_snapshot;
use app_lib::connector::channel::router::ChannelSessionRouter;
use app_lib::connector::channel::types::ConversationType;
use app_lib::runtime::store::{ConversationStore, InMemoryConversationStore};
use tempfile::TempDir;

#[test]
fn legacy_v1_sessions_trigger_full_wipe_on_startup() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("channels/dingtalk/sessions.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();

    // 模拟 v1：无 schemaVersion，key 没有 robot_code 段
    let v1 = serde_json::json!({
        "sessions": {
            "group:cid-A": "sess-legacy-1",
            "private:user-B": "sess-legacy-2",
        }
    });
    std::fs::write(&path, serde_json::to_string_pretty(&v1).unwrap()).unwrap();

    let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());
    conv_store.create_conversation("sess-legacy-1", "old1").unwrap();
    conv_store.create_conversation("sess-legacy-2", "old2").unwrap();

    let router = ChannelSessionRouter::migrate_or_load(&path, conv_store.as_ref()).unwrap();

    assert!(router.entries().is_empty(), "router should be empty after wipe");
    assert!(
        conv_store.list_conversation_ids().unwrap().is_empty(),
        "conversation store should be empty after wipe"
    );

    let raw = std::fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(parsed["schemaVersion"], 2);
    assert!(parsed["sessions"].as_object().unwrap().is_empty());
}

#[test]
fn hydrate_populates_conversations_from_router() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("channels/dingtalk/sessions.json");
    let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());

    // 写入 v2 数据
    {
        let mut router = ChannelSessionRouter::load_for_test(&path).unwrap();
        let _ = router
            .get_or_create_session(&ConversationType::Private, "robot-current", "user-1", || {
                conv_store.create_conversation("sess-1", "姚斌权").unwrap();
                Ok("sess-1".to_string())
            })
            .unwrap();
        let _ = router
            .get_or_create_session(&ConversationType::Group, "robot-current", "cid-2", || {
                conv_store
                    .create_conversation("sess-2", "钉钉群 cid-2")
                    .unwrap();
                Ok("sess-2".to_string())
            })
            .unwrap();
    }

    let router = ChannelSessionRouter::migrate_or_load(&path, conv_store.as_ref()).unwrap();
    let snapshot = build_conversation_snapshot(
        &router.entries(),
        conv_store.as_ref(),
        Some("robot-current"),
    );

    assert_eq!(snapshot.len(), 2);
    assert!(snapshot.iter().all(|c| c.is_active_robot));
    let names: std::collections::HashSet<_> =
        snapshot.iter().map(|c| c.display_name.clone()).collect();
    assert!(names.contains("姚斌权"));
    assert!(names.contains("钉钉群 cid-2"));
}

#[test]
fn hydrate_marks_only_current_robot_as_active() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("channels/dingtalk/sessions.json");
    let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());

    {
        let mut router = ChannelSessionRouter::load_for_test(&path).unwrap();
        router
            .get_or_create_session(&ConversationType::Private, "robot-A", "user-1", || {
                conv_store.create_conversation("sess-a", "user-A").unwrap();
                Ok("sess-a".to_string())
            })
            .unwrap();
        router
            .get_or_create_session(&ConversationType::Private, "robot-B", "user-2", || {
                conv_store.create_conversation("sess-b", "user-B").unwrap();
                Ok("sess-b".to_string())
            })
            .unwrap();
    }

    let router = ChannelSessionRouter::migrate_or_load(&path, conv_store.as_ref()).unwrap();
    let snapshot =
        build_conversation_snapshot(&router.entries(), conv_store.as_ref(), Some("robot-A"));

    let active: Vec<_> = snapshot.iter().filter(|c| c.is_active_robot).collect();
    let inactive: Vec<_> = snapshot.iter().filter(|c| !c.is_active_robot).collect();

    assert_eq!(active.len(), 1);
    assert_eq!(active[0].robot_code, "robot-A");
    assert_eq!(inactive.len(), 1);
    assert_eq!(inactive[0].robot_code, "robot-B");
}

#[test]
fn hydrate_with_no_current_robot_marks_all_inactive() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("channels/dingtalk/sessions.json");
    let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());

    {
        let mut router = ChannelSessionRouter::load_for_test(&path).unwrap();
        router
            .get_or_create_session(&ConversationType::Private, "robot-old", "user-1", || {
                conv_store.create_conversation("sess-1", "old-user").unwrap();
                Ok("sess-1".to_string())
            })
            .unwrap();
    }

    let router = ChannelSessionRouter::migrate_or_load(&path, conv_store.as_ref()).unwrap();
    let snapshot = build_conversation_snapshot(&router.entries(), conv_store.as_ref(), None);

    assert_eq!(snapshot.len(), 1);
    assert!(!snapshot[0].is_active_robot);
}
```

- [ ] **Step 3: 给 router.rs 加 `load_for_test` 公开入口（仅 cfg(test) / feature 受限）**

集成测试不能用 `#[cfg(test)] pub fn load`（因为 `cfg(test)` 在 `lib` crate 编译时是 false）。把 router.rs 里的：

```rust
    #[cfg(test)]
    pub fn load(sessions_path: &Path) -> Result<Self> {
```

改为：

```rust
    /// Public test-only entry: 不做迁移，纯加载。生产代码请用 `migrate_or_load`。
    pub fn load_for_test(sessions_path: &Path) -> Result<Self> {
```

把内部 mod tests 里所有 `ChannelSessionRouter::load(` 改为 `ChannelSessionRouter::load_for_test(`。

跑：

```bash
cd src-tauri && cargo test --lib channel::router 2>&1 | tail -20
```

预期：8 个 router 单测全 PASS。

- [ ] **Step 4: 跑集成测试**

```bash
cd src-tauri && cargo test --test channel_hydrate_test 2>&1 | tail -30
```

预期：4 个测试全 PASS。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/connector/channel/router.rs src-tauri/src/connector/channel/manager.rs src-tauri/tests/channel_hydrate_test.rs
git commit -m "test(channel): hydrate / migration 集成测试

- router::load_for_test 替代 #[cfg(test)] pub fn load（让集成测试可调）
- channel_hydrate_test.rs 覆盖 v1 wipe / hydrate / 多 robot 隔离 / 无 current robot"
```

---

## Task 9：前端 lib/tauri.ts 同步类型字段

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: 找到 ChannelConversation 类型并扩展**

```bash
grep -n "ChannelConversation" src/lib/tauri.ts
```

定位到 `ChannelConversation` 接口定义（约在中段），在 `unreadCount: number` 之后加两个字段：

```ts
export interface ChannelConversation {
  sessionId: string
  platform: ChannelPlatform
  conversationType: 'group' | 'private'
  externalId: string
  displayName: string
  unreadCount: number
  robotCode: string
  isActiveRobot: boolean
}
```

（具体形态以现有接口为准，只加 `robotCode` 和 `isActiveRobot` 两行）

- [ ] **Step 2: 类型检查**

```bash
pnpm exec tsc --noEmit 2>&1 | head -30
```

预期：可能有用到 `ChannelConversation` 字面量构造的地方报"missing properties"，记下下一步处理。

- [ ] **Step 3: 提交**

```bash
git add src/lib/tauri.ts
git commit -m "types(channel): ChannelConversation 加 robotCode/isActiveRobot"
```

---

## Task 10：channelStore.ts —— initChannelListeners 拉对话；platform-state 回调拉一次

**Files:**
- Modify: `src/stores/channelStore.ts:146-161`

- [ ] **Step 1: 写失败测试 — initChannelListeners 调 loadConversations**

打开 `src/stores/channelStore.test.ts`，在文件末尾（最后一个 `})` 之前的同级位置）加：

```ts
  it('initChannelListeners triggers loadConversations on startup', async () => {
    const { initChannelListeners, useChannelStore } = await import('./channelStore')
    const channelGetConversationsMock = (
      await import('@/lib/tauri')
    ).channelGetConversations as ReturnType<typeof vi.fn>

    channelGetConversationsMock.mockResolvedValue([])
    await initChannelListeners()

    expect(channelGetConversationsMock).toHaveBeenCalled()
  })

  it('platform-state event triggers loadConversations refresh', async () => {
    const tauriMod = await import('@/lib/tauri')
    const onChannelPlatformStateMock = tauriMod.onChannelPlatformState as ReturnType<typeof vi.fn>
    const channelGetConversationsMock = tauriMod.channelGetConversations as ReturnType<typeof vi.fn>

    let capturedHandler: ((p: any) => void) | null = null
    onChannelPlatformStateMock.mockImplementation((handler: (p: any) => void) => {
      capturedHandler = handler
      return Promise.resolve(() => {})
    })
    channelGetConversationsMock.mockResolvedValue([])

    const { initChannelListeners } = await import('./channelStore')
    await initChannelListeners()

    channelGetConversationsMock.mockClear()
    capturedHandler?.({
      state: {
        platform: 'dingtalk',
        capability: 'available',
        configured: true,
        enabled: true,
        connection: 'connected',
        config: null,
        lastConnectedAt: null,
        lastError: null,
      },
    })

    await new Promise((r) => setTimeout(r, 0))
    expect(channelGetConversationsMock).toHaveBeenCalled()
  })
```

注：现有 test 文件可能已经有 `vi.mock('@/lib/tauri', ...)` setup；如果 mock 里没列 `channelGetConversations`，要补上。先把测试加进去看 mock 形态。

- [ ] **Step 2: 跑测试，确认失败**

```bash
pnpm exec vitest run src/stores/channelStore.test.ts 2>&1 | tail -30
```

预期：两个新测试 FAIL（`channelGetConversations` 没被调用，因为现有 `initChannelListeners` 只调了 `loadPlatforms`）。

如果"initChannelListeners 只能跑一次"导致测试串扰，需要在每个测试前 reset listenersInitialized——临时方案是把测试文件顶部加 `beforeEach(() => { vi.resetModules() })` 让每次重新 import 模块。

- [ ] **Step 3: 改 initChannelListeners**

把 `src/stores/channelStore.ts:146-161` 整段替换：

```ts
let listenersInitialized = false

/** App 启动时调用一次，订阅后端事件并拉取初始状态 */
export async function initChannelListeners() {
  if (listenersInitialized) return
  listenersInitialized = true

  await useChannelStore.getState().loadPlatforms()
  await useChannelStore.getState().loadConversations()

  await onChannelPlatformState(({ state }) => {
    useChannelStore.getState().setPlatformState(state)
    // refresh_active_robot_flags 改了 is_active_robot 但没单独的 conversations 事件，
    // 所以这里要主动拉一次新快照（remove / reconnect / 切换机器人都走这条）。
    void useChannelStore.getState().loadConversations()
  })
  await onChannelMessage(({ sessionId }) => {
    const { activeSessionId } = useChannelStore.getState()
    if (sessionId !== activeSessionId) {
      useChannelStore.getState().incrementUnread(sessionId)
    }
  })
}
```

- [ ] **Step 4: 跑测试**

```bash
pnpm exec vitest run src/stores/channelStore.test.ts 2>&1 | tail -30
```

预期：全部 PASS。

- [ ] **Step 5: 提交**

```bash
git add src/stores/channelStore.ts src/stores/channelStore.test.ts
git commit -m "fix(channel/store): 启动 + platform-state 触发 loadConversations

- initChannelListeners 启动后立即拉一次 conversations（修复重启后空白 bug）
- onChannelPlatformState 回调里再拉一次（同步 refresh_active_robot_flags 的副作用）"
```

---

## Task 11：AppSidebar.tsx 折叠区 + 文案逻辑

**Files:**
- Modify: `src/components/sidebar/AppSidebar.tsx:165-227`

- [ ] **Step 1: 写组件测试（失败）**

新建 `src/components/sidebar/AppSidebar.test.tsx`：

```tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, it, expect, beforeEach, vi } from 'vitest'

import { AppSidebar } from './AppSidebar'
import { useChannelStore } from '@/stores/channelStore'
import type { ChannelConversation } from '@/lib/tauri'

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({
    conversations: [],
    activeConversationId: null,
    switchConversation: vi.fn(),
    renameConversation: vi.fn(),
    archiveConversation: vi.fn(),
  }),
}))

vi.mock('@/stores/authStore', () => ({
  useAuthStore: (sel: any) => sel({ tenant: { name: 'T' } }),
}))

vi.mock('@/stores/brandingStore', () => ({
  useBrandingStore: (sel: any) => sel({ productName: 'AIjia', logoUrl: '' }),
}))

vi.mock('@/stores/uiStore', () => ({
  useUiStore: (sel: any) =>
    sel({ route: { kind: 'home' }, setRoute: vi.fn(), openSettings: vi.fn() }),
}))

const conv = (overrides: Partial<ChannelConversation>): ChannelConversation => ({
  sessionId: 's',
  platform: 'dingtalk',
  conversationType: 'private',
  externalId: 'ext',
  displayName: 'name',
  unreadCount: 0,
  robotCode: 'robot-1',
  isActiveRobot: true,
  ...overrides,
})

describe('AppSidebar 频道区', () => {
  beforeEach(() => {
    useChannelStore.setState({
      platforms: {
        dingtalk: {
          platform: 'dingtalk',
          capability: 'available',
          configured: true,
          enabled: true,
          connection: 'connected',
          config: null,
          lastConnectedAt: null,
          lastError: null,
        } as any,
      },
      conversations: [],
      activeSessionId: null,
    })
  })

  const switchToChannelTab = () => fireEvent.click(screen.getByRole('button', { name: /频道/ }))

  it('活跃 0 + legacy 0 → 显示 "未配置，点击右侧设置"', () => {
    useChannelStore.setState({
      platforms: {
        dingtalk: {
          platform: 'dingtalk',
          capability: 'available',
          configured: false,
          enabled: false,
          connection: 'unconfigured',
          config: null,
          lastConnectedAt: null,
          lastError: null,
        } as any,
      },
      conversations: [],
    })
    render(<AppSidebar />)
    switchToChannelTab()
    expect(screen.getByText('未配置，点击右侧设置')).toBeInTheDocument()
    expect(screen.queryByText(/历史会话/)).not.toBeInTheDocument()
  })

  it('活跃 0 + legacy >0 → 显示 "未配置" + 折叠按钮', () => {
    useChannelStore.setState({
      conversations: [
        conv({ sessionId: 's1', displayName: '老用户A', isActiveRobot: false, robotCode: 'old-A' }),
        conv({ sessionId: 's2', displayName: '老用户B', isActiveRobot: false, robotCode: 'old-A' }),
      ],
    })
    render(<AppSidebar />)
    switchToChannelTab()
    expect(screen.getByText('未配置，点击右侧设置')).toBeInTheDocument()
    expect(screen.getByText(/历史会话/)).toBeInTheDocument()
    expect(screen.queryByText('老用户A')).not.toBeInTheDocument() // 默认折叠
  })

  it('活跃 >0 + legacy >0 → 顶部活跃列表 + 底部折叠按钮', () => {
    useChannelStore.setState({
      conversations: [
        conv({ sessionId: 's1', displayName: '姚斌权', isActiveRobot: true, robotCode: 'cur' }),
        conv({ sessionId: 's2', displayName: '老用户', isActiveRobot: false, robotCode: 'old' }),
      ],
    })
    render(<AppSidebar />)
    switchToChannelTab()
    expect(screen.getByText('姚斌权')).toBeInTheDocument()
    expect(screen.getByText(/历史会话/)).toBeInTheDocument()
  })

  it('点击折叠按钮展开 → legacy 对话按 robotCode 二级分组显示', () => {
    useChannelStore.setState({
      conversations: [
        conv({ sessionId: 's1', displayName: '张三', isActiveRobot: false, robotCode: 'robot-old-001' }),
        conv({ sessionId: 's2', displayName: '李四', isActiveRobot: false, robotCode: 'robot-old-001' }),
        conv({ sessionId: 's3', displayName: '王五', isActiveRobot: false, robotCode: 'robot-old-002' }),
      ],
    })
    render(<AppSidebar />)
    switchToChannelTab()
    fireEvent.click(screen.getByText(/历史会话/))
    expect(screen.getByText('张三')).toBeInTheDocument()
    expect(screen.getByText('李四')).toBeInTheDocument()
    expect(screen.getByText('王五')).toBeInTheDocument()
    // 二级分组标题至少出现 2 次（两个 robotCode）
    expect(screen.getAllByText(/robot-old/).length).toBeGreaterThanOrEqual(2)
  })

  it('活跃 >0 + legacy 0 → 仅活跃列表，不显示折叠按钮', () => {
    useChannelStore.setState({
      conversations: [
        conv({ sessionId: 's1', displayName: '姚斌权', isActiveRobot: true, robotCode: 'cur' }),
      ],
    })
    render(<AppSidebar />)
    switchToChannelTab()
    expect(screen.getByText('姚斌权')).toBeInTheDocument()
    expect(screen.queryByText(/历史会话/)).not.toBeInTheDocument()
  })
})
```

- [ ] **Step 2: 跑测试，确认失败**

```bash
pnpm exec vitest run src/components/sidebar/AppSidebar.test.tsx 2>&1 | tail -40
```

预期：所有断言失败（折叠区还没实现，所有对话都被扁平渲染）。

- [ ] **Step 3: 改 AppSidebar.tsx 实现折叠区**

打开 `src/components/sidebar/AppSidebar.tsx`，在 `import` 区加 `ChevronRight` icon：

```tsx
import { CheckSquare, ChevronRight, MessageSquare } from 'lucide-react'
```

在 `const [archivingId, ...]` 之后加：

```tsx
  const [legacyExpanded, setLegacyExpanded] = useState(false)

  const activeConversations = channelConversations.filter((c) => c.isActiveRobot)
  const legacyConversations = channelConversations.filter((c) => !c.isActiveRobot)
  const legacyByRobot = legacyConversations.reduce<Record<string, typeof channelConversations>>(
    (acc, c) => {
      ;(acc[c.robotCode] ??= []).push(c)
      return acc
    },
    {},
  )

  const dingtalkConfigured = !!(dingtalkState?.configured && dingtalkState.enabled)
```

把当前 `<div className="border-l border-border pl-3">` 整个块（line 177-207）替换为下面三段渲染：

```tsx
                <div className="border-l border-border pl-3">
                  {!dingtalkConfigured && activeConversations.length === 0 ? (
                    <button
                      type="button"
                      onClick={openChannelOverview}
                      className="w-full rounded-lg px-3 py-2 text-left text-sm font-medium text-muted-foreground hover:bg-sidebar-accent/50 hover:text-sidebar-foreground"
                    >
                      未配置，点击右侧设置
                    </button>
                  ) : (
                    activeConversations.map((conversation) => (
                      <button
                        key={conversation.sessionId}
                        type="button"
                        onClick={() => selectChannelSession(conversation.sessionId)}
                        className={
                          channelActiveSessionId === conversation.sessionId
                            ? 'flex w-full items-center justify-between rounded-lg bg-sidebar-accent px-3 py-2 text-left text-sm font-semibold text-sidebar-foreground'
                            : 'flex w-full items-center justify-between rounded-lg px-3 py-2 text-left text-sm font-medium text-sidebar-foreground/70 hover:bg-sidebar-accent/50 hover:text-sidebar-foreground'
                        }
                      >
                        <span className="truncate">{conversation.displayName}</span>
                        {conversation.unreadCount > 0 && (
                          <span className="ml-2 rounded-full bg-primary px-1.5 text-xs text-primary-foreground">
                            {conversation.unreadCount}
                          </span>
                        )}
                      </button>
                    ))
                  )}
                </div>

                {legacyConversations.length > 0 && (
                  <div className="mt-2 border-l border-border pl-3">
                    <button
                      type="button"
                      onClick={() => setLegacyExpanded((v) => !v)}
                      className="flex w-full items-center gap-1 rounded-lg px-3 py-1.5 text-left text-xs font-medium text-muted-foreground hover:bg-sidebar-accent/50"
                    >
                      <ChevronRight
                        className={`h-3 w-3 transition-transform ${legacyExpanded ? 'rotate-90' : ''}`}
                      />
                      历史会话 ({legacyConversations.length})
                    </button>
                    {legacyExpanded && (
                      <div className="mt-1 flex flex-col gap-1">
                        {Object.entries(legacyByRobot).map(([robotCode, list]) => (
                          <div key={robotCode}>
                            <div className="px-3 py-1 text-[11px] font-medium text-muted-foreground/70">
                              {robotCode.length > 14 ? `${robotCode.slice(0, 14)}…` : robotCode}
                            </div>
                            {list.map((conversation) => (
                              <button
                                key={conversation.sessionId}
                                type="button"
                                onClick={() => selectChannelSession(conversation.sessionId)}
                                className="flex w-full items-center justify-between rounded-lg px-3 py-1.5 text-left text-sm text-muted-foreground hover:bg-sidebar-accent/50"
                              >
                                <span className="truncate">{conversation.displayName}</span>
                              </button>
                            ))}
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                )}
```

注意原来 `<div className="border-l border-border pl-3">` 上方有 `<div>{...钉钉标题徽标}</div>`，保留不动。新增的"历史会话"块作为同级元素插入。

- [ ] **Step 4: 跑测试**

```bash
pnpm exec vitest run src/components/sidebar/AppSidebar.test.tsx 2>&1 | tail -40
```

预期：5 个测试全 PASS。

- [ ] **Step 5: 跑 lint + tsc**

```bash
pnpm lint 2>&1 | tail -10
pnpm exec tsc --noEmit 2>&1 | tail -10
```

预期：无错。

- [ ] **Step 6: 提交**

```bash
git add src/components/sidebar/AppSidebar.tsx src/components/sidebar/AppSidebar.test.tsx
git commit -m "feat(sidebar): 钉钉历史会话折叠区

- isActiveRobot=false 的对话进折叠区
- 折叠区按 robotCode 二级分组（前 14 字符截断）
- 折叠状态本地 state，刷新重置
- 用主题变量颜色（text-muted-foreground / border-border）"
```

---

## Task 12：ChannelPage.tsx — inactive session 输入区禁用 + banner

**Files:**
- Modify: `src/features/channel/ChannelPage.tsx`
- Modify: `src/features/channel/ChannelPage.test.tsx`

- [ ] **Step 1: 找到 ChannelPage 输入区代码**

```bash
grep -n "input\|disabled\|sessionId\|activeSessionId\|sendChannelMessage" src/features/channel/ChannelPage.tsx | head -30
```

定位输入区 textarea / Input 元素 和 send 按钮。

- [ ] **Step 2: 写测试（失败）**

打开 `src/features/channel/ChannelPage.test.tsx`，加：

```tsx
  it('inactive 会话：输入区 disabled + banner 提示', async () => {
    useChannelStore.setState({
      conversations: [
        {
          sessionId: 'sess-old',
          platform: 'dingtalk',
          conversationType: 'private',
          externalId: 'u',
          displayName: '老用户',
          unreadCount: 0,
          robotCode: 'old-robot',
          isActiveRobot: false,
        },
      ],
      activeSessionId: 'sess-old',
    })
    render(<ChannelPage />)
    expect(
      screen.getByText(/已下线的机器人，无法发送新消息/),
    ).toBeInTheDocument()
    // 输入区 textarea 应该 disabled（具体 selector 按现有实现调整）
    const textarea = screen.queryByPlaceholderText(/输入|发送|消息/i)
    if (textarea) {
      expect(textarea).toBeDisabled()
    }
  })

  it('active 会话：输入区可用，无 banner', async () => {
    useChannelStore.setState({
      conversations: [
        {
          sessionId: 'sess-cur',
          platform: 'dingtalk',
          conversationType: 'private',
          externalId: 'u',
          displayName: '姚斌权',
          unreadCount: 0,
          robotCode: 'current-robot',
          isActiveRobot: true,
        },
      ],
      activeSessionId: 'sess-cur',
    })
    render(<ChannelPage />)
    expect(
      screen.queryByText(/已下线的机器人，无法发送新消息/),
    ).not.toBeInTheDocument()
  })
```

注：现有 ChannelPage.test.tsx 的 setup 方式（mock provider、router 状态）以现有文件为准；如果 setState 不够，参考现有用例的写法补全。

- [ ] **Step 3: 跑测试，确认失败**

```bash
pnpm exec vitest run src/features/channel/ChannelPage.test.tsx 2>&1 | tail -30
```

预期：两个新测试 FAIL。

- [ ] **Step 4: 在 ChannelPage 实现 inactive 判断**

在 `ChannelPage.tsx` 组件函数内（hooks 区域，靠近 `activeSessionId` 派生处）加：

```tsx
  const conversations = useChannelStore((s) => s.conversations)
  const activeConv = conversations.find((c) => c.sessionId === activeSessionId)
  const isInactiveSession = activeConv && !activeConv.isActiveRobot
```

在输入区的容器顶部（输入框 / 发送按钮之上）加 banner：

```tsx
{isInactiveSession && (
  <div className="mb-2 rounded-md bg-muted px-3 py-2 text-sm text-muted-foreground">
    该会话来自已下线的机器人，无法发送新消息
  </div>
)}
```

把输入区 textarea / Input 加 `disabled={isInactiveSession}`，发送按钮 `disabled={isInactiveSession || ...原有条件}`。

- [ ] **Step 5: 跑测试**

```bash
pnpm exec vitest run src/features/channel/ChannelPage.test.tsx 2>&1 | tail -30
```

预期：全部 PASS。

- [ ] **Step 6: 跑 lint + tsc**

```bash
pnpm lint 2>&1 | tail -5
pnpm exec tsc --noEmit 2>&1 | tail -5
```

- [ ] **Step 7: 提交**

```bash
git add src/features/channel/ChannelPage.tsx src/features/channel/ChannelPage.test.tsx
git commit -m "feat(channel/page): inactive session 输入区禁用 + banner

isActiveRobot=false 的会话进入聊天页时：
- 顶部显示 banner 提示机器人已下线
- 输入框 / 发送按钮 disabled"
```

---

## Task 13：全量回归 + 手测验收

**Files:** 无

- [ ] **Step 1: 全量后端测试**

```bash
cd src-tauri && cargo test --no-fail-fast 2>&1 | tail -40
```

预期：全部 PASS（含 review_ 系列）。

- [ ] **Step 2: 全量前端关键测试**

```bash
pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts src/stores/channelStore.test.ts src/components/sidebar/AppSidebar.test.tsx src/features/channel/ChannelPage.test.tsx 2>&1 | tail -30
```

预期：全部 PASS。

- [ ] **Step 3: lint + tsc 全量**

```bash
pnpm lint 2>&1 | tail -10
pnpm exec tsc --noEmit 2>&1 | tail -10
```

- [ ] **Step 4: 启 dev 跑手测**

```bash
pnpm tauri:dev
```

按以下顺序手测，每条对应 spec §测试策略汇总 的一项：

1. **干净启动**：`rm -rf ~/.renlijia/.../channels/dingtalk/sessions.json` + 删除当前 conversation 目录 → 启动 → 配机器人 → 钉钉发消息 → 侧边栏看到对话
2. **bug 修复**：完全退出 App → 重启 → **不点 ChannelPage**，直接点侧边栏「频道」标签 → 应该看到老对话（验证 hydrate 生效）
3. **切换式核心**：钉钉发消息建对话 → UI 移除机器人 → 对话进折叠区，灰色 → 点开能看历史 → 进入 ChannelPage 看到 banner + 输入区 disabled
4. **重连同机器人**：重新填同 app_key/secret → connect → 折叠区对话回到活跃区
5. **切到新机器人**：注册另一个机器人（用 manual / 扫码新应用）→ 老对话留在折叠区 → 新机器人发消息建新对话在活跃区
6. **schema 迁移**：把当前 sessions.json 临时手改回 v1（删 schemaVersion 字段、把 key 改回 `group:cid` 格式），保留 conversation 目录 → 重启 App → 验证 sessions.json 变 v2 空 + conversation 目录被删 + 侧边栏显示"未配置"

- [ ] **Step 5: 提交（手测无问题，无新代码改动则跳过）**

如果手测发现 bug 回到对应 task 修；全部通过则进入 finishing-a-development-branch。

---

## 自查（plan author 完成后核对）

- [x] 所有任务对齐 spec 章节（§数据模型 / §后端改动 / §前端改动 / §测试策略汇总 / §边界情况）
- [x] 没有 TBD / TODO / "implement later" / "适当的错误处理"
- [x] 类型一致：`migrate_or_load` / `entries` / `RouterEntry` / `build_conversation_snapshot` / `hydrate_conversations` / `refresh_active_robot_flags` 在所有 task 中签名一致
- [x] 每个 step 都有具体代码或具体命令
- [x] 提交粒度合理（每个 task 一次 commit，且 commit message 描述清晰）
