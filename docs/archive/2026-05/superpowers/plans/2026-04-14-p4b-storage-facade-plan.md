# P4-B AppStorage → ConversationStore Facade 迁移计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `runtime/conversation_service.rs` 中 4 个直接依赖 `Arc<AppStorage>` 的函数改为依赖 `Arc<dyn ConversationStore>`，断开 runtime 层对 file-store 实现的直接耦合。

**Architecture:** `ConversationStore` trait 已在 `runtime/store/conversation_store.rs` 中定义并有 `AppStorage` impl，只需把 `conversation_service.rs` 的函数签名中 `Arc<AppStorage>` 替换为 `Arc<dyn ConversationStore>`，同时更新所有调用点（tauri_commands 层）。`delete_conversation` 中有 `db.get_file_paths_for_conversation` 和 `db.delete_memories_by_prefix` 两个方法不在 trait 里，需要单独处理（扩展 trait 或拆函数）。

**Tech Stack:** Rust, existing `ConversationStore` trait (`runtime/store/conversation_store.rs`)

---

## 当前状态

`conversation_service.rs` 现有 4 个使用 `Arc<AppStorage>` 的函数：

| 函数 | AppStorage 方法 | 是否在 ConversationStore trait |
|------|----------------|-------------------------------|
| `get_messages` | `db.get_messages` | ✅ 是 |
| `create_conversation` | `db.create_conversation` | ✅ 是 |
| `get_conversations` | `db.get_conversations` | ✅ 是 |
| `rename_conversation` | `db.update_conversation_title` | ❌ 否（需添加 `rename_conversation` 方法） |
| `delete_conversation` | 多个方法，含 `get_file_paths_for_conversation`、`delete_memories_by_prefix`、`remove_active_task`、`delete_conversation` | 部分在 trait，部分不在 |

**结论：**
- `get_messages`、`create_conversation`、`get_conversations` → 可直接改签名
- `rename_conversation` → 需先在 trait 中添加 `rename_conversation` 方法
- `delete_conversation` → 复杂，含 file 清理 + memory 清理，保留 `Arc<AppStorage>` 或拆出 domain-specific trait 方法

> **范围决策：** `delete_conversation` 涉及文件系统清理（file_mgr）和 memory 清理，这些不属于"对话生命周期"的纯 domain trait。本计划只迁移 3 个简单函数，`delete_conversation` 保持现状，待 P4 后续阶段处理。

---

## 文件变更清单

| 文件 | 操作 |
|------|------|
| `src-tauri/src/runtime/store/conversation_store.rs` | 添加 `rename_conversation` 方法到 trait + `InMemoryConversationStore` impl + `AppStorage` impl |
| `src-tauri/src/runtime/conversation_service.rs` | 修改 `get_messages`、`create_conversation`、`get_conversations`、`rename_conversation` 签名 |
| `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs` 或调用点 | 更新调用点（`Arc<AppStorage>` 改为 `Arc<dyn ConversationStore>`） |

---

## Task 1：在 ConversationStore trait 中添加 `rename_conversation`

**文件：**
- Modify: `src-tauri/src/runtime/store/conversation_store.rs`

- [ ] **Step 1：在 trait 中添加方法**

在 `conversation_store.rs` 的 `ConversationStore` trait（约 L9-26）中，在 `get_messages` 之后添加：

```rust
/// Rename an existing conversation.
fn rename_conversation(&self, id: &str, new_title: &str) -> Result<()>;
```

- [ ] **Step 2：在 `InMemoryConversationStore` 中实现**

在 `InMemoryConversationStore` 的 `impl ConversationStore` 块中添加：

```rust
fn rename_conversation(&self, id: &str, new_title: &str) -> Result<()> {
    let mut convs = self.conversations.lock().unwrap();
    if convs.contains_key(id) {
        convs.insert(id.to_string(), new_title.to_string());
        Ok(())
    } else {
        anyhow::bail!("conversation '{}' not found", id)
    }
}
```

- [ ] **Step 3：为 `AppStorage` 添加 `ConversationStore` impl 的 `rename_conversation`**

搜索 `AppStorage` 的 `ConversationStore` impl（在 `storage/file_store/` 中用 `grep -rn "impl ConversationStore" src-tauri/src/` 找到），添加：

```rust
fn rename_conversation(&self, id: &str, new_title: &str) -> Result<()> {
    self.update_conversation_title(id, new_title)
}
```

- [ ] **Step 4：编译验证**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo check 2>&1 | grep "^error" | head -20
```

期望：无 error（如有 error 说明 AppStorage impl 位置找错，重新找）。

- [ ] **Step 5：写测试确认 InMemory 实现正确**

在 `conversation_store.rs` 末尾的 `#[cfg(test)]` 块中（若有，否则新建），添加：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inmemory_rename_conversation() {
        let store = InMemoryConversationStore::new();
        store.create_conversation("c1", "Old Title").unwrap();
        store.rename_conversation("c1", "New Title").unwrap();
        let convs = store.get_conversations().unwrap();
        // 至少有一个条目
        assert!(!convs.is_empty());
    }

    #[test]
    fn test_inmemory_rename_nonexistent_fails() {
        let store = InMemoryConversationStore::new();
        let result = store.rename_conversation("nonexistent", "Title");
        assert!(result.is_err());
    }
}
```

- [ ] **Step 6：运行新测试**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test conversation_store -- --nocapture 2>&1 | tail -15
```

期望：两个新测试全绿。

- [ ] **Step 7：提交**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add src-tauri/src/runtime/store/conversation_store.rs
git commit -m "feat(store): add rename_conversation to ConversationStore trait and impls"
```

---

## Task 2：迁移 conversation_service.rs 中 3 个简单函数

**文件：**
- Modify: `src-tauri/src/runtime/conversation_service.rs`

- [ ] **Step 1：修改 `use` 引用，添加 trait import**

在 `conversation_service.rs` 顶部修改：

```rust
use std::sync::Arc;

use crate::llm::gateway::LlmGateway;
use crate::runtime::store::conversation_store::ConversationStore;
use crate::storage::file_manager::FileManager;
use crate::storage::file_store::AppStorage;  // 保留：delete_conversation 仍需要
```

- [ ] **Step 2：修改 `get_messages` 签名**

将：
```rust
pub async fn get_messages(
    db: Arc<AppStorage>,
    conversation_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    db.get_messages(&conversation_id).map_err(|e| e.to_string())
}
```

改为：
```rust
pub async fn get_messages(
    db: Arc<dyn ConversationStore>,
    conversation_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    db.get_messages(&conversation_id).map_err(|e| e.to_string())
}
```

- [ ] **Step 3：修改 `create_conversation` 签名**

将：
```rust
pub async fn create_conversation(db: Arc<AppStorage>) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db.create_conversation(&id, "New Conversation")
        .map_err(|e| e.to_string())?;
    Ok(id)
}
```

改为：
```rust
pub async fn create_conversation(db: Arc<dyn ConversationStore>) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db.create_conversation(&id, "New Conversation")
        .map_err(|e| e.to_string())?;
    Ok(id)
}
```

- [ ] **Step 4：修改 `rename_conversation` 签名**

将：
```rust
pub async fn rename_conversation(
    db: Arc<AppStorage>,
    conversation_id: String,
    new_title: String,
) -> Result<RenameConversationOutcome, String> {
    db.update_conversation_title(&conversation_id, &new_title)
        .map_err(|e| e.to_string())?;
    Ok(RenameConversationOutcome {
        conversation_id,
        new_title,
    })
}
```

改为：
```rust
pub async fn rename_conversation(
    db: Arc<dyn ConversationStore>,
    conversation_id: String,
    new_title: String,
) -> Result<RenameConversationOutcome, String> {
    db.rename_conversation(&conversation_id, &new_title)
        .map_err(|e| e.to_string())?;
    Ok(RenameConversationOutcome {
        conversation_id,
        new_title,
    })
}
```

- [ ] **Step 5：修改 `get_conversations` 签名**

将：
```rust
pub async fn get_conversations(db: Arc<AppStorage>) -> Result<Vec<serde_json::Value>, String> {
    db.get_conversations().map_err(|e| e.to_string())
}
```

改为：
```rust
pub async fn get_conversations(db: Arc<dyn ConversationStore>) -> Result<Vec<serde_json::Value>, String> {
    db.get_conversations().map_err(|e| e.to_string())
}
```

- [ ] **Step 6：编译，看 error 定位调用点**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo check 2>&1 | grep "^error" | head -30
```

期望：出现类型不匹配 error，指向调用点（tauri_commands 层），记下文件名和行号。

- [ ] **Step 7：提交函数签名变更（即使编译还未通过）**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add src-tauri/src/runtime/conversation_service.rs
git commit -m "refactor(service): use ConversationStore trait in get_messages/create/rename/get_conversations"
```

---

## Task 3：更新 tauri_commands 层调用点

**文件：**
- Modify: 调用 `conversation_service::get_messages` 等函数的 tauri commands 文件

tauri_commands 层传递的是 `Arc<AppStorage>`，但 `AppStorage` 已实现 `ConversationStore`，所以只需把传入的 `Arc<AppStorage>` 显式转换为 `Arc<dyn ConversationStore>`。

- [ ] **Step 1：定位所有调用点**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && grep -rn "conversation_service::\(get_messages\|create_conversation\|rename_conversation\|get_conversations\)" src-tauri/src/ | grep -v "conversation_service.rs"
```

记录所有调用点文件和行号。

- [ ] **Step 2：在每个调用点，将 `db.clone()` 改为 `db.clone() as Arc<dyn ConversationStore>`**

对每个调用点，例如：
```rust
// 修改前
conversation_service::get_messages(db.clone(), conversation_id).await

// 修改后
conversation_service::get_messages(
    db.clone() as Arc<dyn crate::runtime::store::conversation_store::ConversationStore>,
    conversation_id,
).await
```

或者在调用文件顶部添加 `use crate::runtime::store::conversation_store::ConversationStore;` 后写：
```rust
conversation_service::get_messages(
    db.clone() as Arc<dyn ConversationStore>,
    conversation_id,
).await
```

- [ ] **Step 3：编译验证全通**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo check 2>&1 | grep "^error" | head -20
```

期望：无 error。

- [ ] **Step 4：运行测试**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --tests -- --no-fail-fast 2>&1 | grep -E "FAILED|^test result" | tail -20
```

期望：无新增 FAILED（已知 Tier B 红灯除外）。

- [ ] **Step 5：提交**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add -A
git commit -m "refactor(commands): pass Arc<dyn ConversationStore> to conversation_service functions"
```

---

## Task 4：验收与 README 更新

- [ ] **Step 1：确认 conversation_service.rs 不再直接依赖 AppStorage（除 delete_conversation）**

```bash
grep -n "AppStorage" /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/conversation_service.rs
```

期望：只有 `delete_conversation` 函数的签名仍使用 `Arc<AppStorage>`，其余行不出现 `AppStorage`。

- [ ] **Step 2：运行完整回归测试**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --tests --no-fail-fast 2>&1 | grep -E "FAILED|test result.*ok" | tail -20
```

期望：已知 Tier B 红灯之外全绿。

- [ ] **Step 3：提交 README 更新**

在 `docs/superpowers/plans/README.md` 的 P4 表格中，将 `AppStorage → repository facade` 的状态改为 `✅ 已关闭（2026-04-14，conversation_service.rs 完成，delete_conversation 待后续）`。

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add docs/superpowers/plans/README.md
git commit -m "docs: mark P4-B storage facade partial closure for conversation_service"
```

---

## 自检

### Spec 覆盖

| 要求 | 对应 Task |
|------|---------|
| conversation_service.rs 迁离 AppStorage | Task 2（3 个函数）+ Task 3（调用点更新） |
| ConversationStore trait 增加 rename 方法 | Task 1 |
| 已有测试覆盖新 trait 方法 | Task 1 Step 5-6 |
| delete_conversation 暂保留（范围说明） | 设计决策，非遗漏 |

### Placeholder 扫描

无 TBD / TODO。调用点 Step 2 给出了完整的代码模式，含 `as Arc<dyn ConversationStore>` 转型写法。

### 类型一致性

`ConversationStore::rename_conversation` 在 Task 1 定义，在 Task 2 Step 4 使用（`db.rename_conversation`），名称一致。
