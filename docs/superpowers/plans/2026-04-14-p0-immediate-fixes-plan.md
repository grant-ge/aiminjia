# P0 立即修复计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 5 个安全漏洞和高危 bug，不涉及架构改造。每个修复点独立，可按任意顺序执行。

**Architecture:** 每个 Task 对应一个独立修复点，互不依赖。修复完成后合并，不需要等待其他 Task。

**Tech Stack:** Rust / Tokio / Python / TypeScript / Tauri v2

---

## Task 1：M1 — Claude provider 多轮 tool calling 损坏

**Problem:** `claude.rs` 消息序列化只发 `role` 和 `content`，完全丢弃 `tool_calls` 和 `tool_call_id` 字段。Anthropic API 要求历史消息中 assistant turn 必须含 `tool_use` content block，user turn 必须含 `tool_result` content block。多轮 tool calling 时 API 返回 400。

**Files:**
- Modify: `src-tauri/src/llm/providers/claude.rs` (第 73-82 行附近，`build_request_body`)
- Reference: `src-tauri/src/llm/providers/openai.rs` (第 111-134 行，正确实现参考)
- Test: `src-tauri/tests/` (新建或修改 claude provider 测试)

- [ ] **Step 1.1: 读取 openai.rs 的正确实现，了解 tool_calls / tool_call_id 的序列化方式**

```bash
sed -n '100,160p' src-tauri/src/llm/providers/openai.rs
```

- [ ] **Step 1.2: 读取 claude.rs 当前的 build_request_body 实现**

```bash
sed -n '60,120p' src-tauri/src/llm/providers/claude.rs
```

- [ ] **Step 1.3: 修复 claude.rs 的消息序列化**

对 assistant role 消息，如果 `msg.tool_calls` 非空，需要将 tool_calls 转换为 Anthropic 格式的 `tool_use` content block：

```rust
// claude.rs build_request_body 中的消息序列化部分
// assistant 消息含 tool calls 时：
if msg.role == "assistant" {
    let mut content_blocks: Vec<serde_json::Value> = Vec::new();
    
    // 如有文本内容，加入 text block
    if !msg.content.is_empty() {
        content_blocks.push(json!({
            "type": "text",
            "text": msg.content
        }));
    }
    
    // 将每个 tool_call 转为 tool_use block
    for tc in &msg.tool_calls {
        content_blocks.push(json!({
            "type": "tool_use",
            "id": tc.id,
            "name": tc.function.name,
            "input": serde_json::from_str::<serde_json::Value>(&tc.function.arguments)
                .unwrap_or(json!({}))
        }));
    }
    
    json!({ "role": "assistant", "content": content_blocks })
}
// user 消息含 tool_call_id 时（tool result）：
else if msg.role == "user" && msg.tool_call_id.is_some() {
    json!({
        "role": "user",
        "content": [{
            "type": "tool_result",
            "tool_use_id": msg.tool_call_id,
            "content": msg.content
        }]
    })
} else {
    json!({ "role": msg.role, "content": msg.content })
}
```

实际字段名需参照当前 `ChatMessage` struct 定义调整。

- [ ] **Step 1.4: 编译验证**

```bash
cd src-tauri && cargo build --lib 2>&1 | grep -E "^error"
```

Expected: 无 error。

- [ ] **Step 1.5: 运行现有 Claude provider 相关测试**

```bash
cd src-tauri && cargo test claude -- --nocapture 2>&1 | tail -20
```

- [ ] **Step 1.6: Commit**

```bash
git add src-tauri/src/llm/providers/claude.rs
git commit -m "fix(llm): fix claude provider multi-turn tool calling

Serialize tool_calls as Anthropic tool_use content blocks and
tool_call_id responses as tool_result blocks. Previously these
fields were dropped causing API 400 errors on multi-turn tool calls.

Ref: P0/M1"
```

---

## Task 2：PY2 — Python 子进程继承父进程全部 env var（含 API key）

**Problem:** `python/mod.rs` 的 `configure_python_env` 只在特定条件下移除 `PYTHONPATH`，子进程完整继承父进程所有 env var。Python 代码可通过 `import os; print(os.environ)` 读取 `ANTHROPIC_API_KEY` 等敏感变量。

**Files:**
- Modify: `src-tauri/src/python/mod.rs` (第 15-25 行附近，`configure_python_env`)
- Test: 验证子进程无法读取敏感 env var

- [ ] **Step 2.1: 读取当前 configure_python_env 实现**

```bash
sed -n '1,60p' src-tauri/src/python/mod.rs
```

- [ ] **Step 2.2: 在 configure_python_env 中显式清除敏感 env var**

在 `Command` 配置中加入 `env_remove` 调用，清除已知敏感变量：

```rust
// 在 configure_python_env 中，Command::new(python_path) 之后加入：
let sensitive_vars = [
    "ANTHROPIC_API_KEY",
    "OPENAI_API_KEY", 
    "TAVILY_API_KEY",
    "BOCHA_API_KEY",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_ACCESS_KEY_ID",
    // 根据实际使用的 env var 补充
];
for var in &sensitive_vars {
    cmd.env_remove(var);
}
```

- [ ] **Step 2.3: 编译验证**

```bash
cd src-tauri && cargo build --lib 2>&1 | grep -E "^error"
```

- [ ] **Step 2.4: Commit**

```bash
git add src-tauri/src/python/mod.rs
git commit -m "fix(python): strip sensitive env vars from python subprocess

Python subprocesses previously inherited all parent env vars,
exposing API keys to user code. Explicitly remove known sensitive
variables before spawning.

Ref: P0/PY2"
```

---

## Task 3：PY3 — `_restore.py` pickle 反序列化 RCE 风险

**Problem:** `_restore.py` 使用 `pickle.load` 无条件加载 `analysis/<conv_id>/*.pkl` 文件。如果 workspace 目录被写入构造的 pickle 文件（通过文件上传功能），可在 restore 时触发任意代码执行，且此路径跳过 `validate_code`。

**Files:**
- Read: `src-tauri/src/python/_restore.py` (完整读取)
- Modify: `src-tauri/src/python/_restore.py`

- [ ] **Step 3.1: 读取当前 _restore.py 实现**

```bash
cat src-tauri/src/python/_restore.py
```

- [ ] **Step 3.2: 替换 pickle 为 JSON 序列化**

**方案 A（推荐）：完全弃用 pickle，改为 JSON**

将 checkpoint 保存和加载都改为 JSON，放弃 pickle 的二进制对象序列化能力。对于 pandas DataFrame，改为保存 CSV + schema JSON：

```python
# 保存 checkpoint（在执行脚本的 epilogue 中）
import json
checkpoint = {
    "variables": {k: _serialize_var(v) for k, v in globals().items() 
                  if not k.startswith('_')},
    "version": 1
}
with open(checkpoint_path, 'w', encoding='utf-8') as f:
    json.dump(checkpoint, f, ensure_ascii=False, default=str)
```

```python
# _restore.py 中加载 checkpoint
import json
with open(pkl_file, 'r', encoding='utf-8') as f:
    checkpoint = json.load(f)
# 恢复变量...
```

**方案 B（最小改动）：加 HMAC 完整性校验**

如果 DataFrame/numpy 对象必须用 pickle，则：

```python
import hmac, hashlib, os

CHECKPOINT_SECRET = os.environ.get('_CHECKPOINT_SECRET', '')

def _safe_load_checkpoint(path):
    sig_path = path + '.sig'
    if not os.path.exists(sig_path):
        raise ValueError(f"Checkpoint signature missing: {sig_path}")
    with open(path, 'rb') as f:
        data = f.read()
    with open(sig_path, 'r') as f:
        expected_sig = f.read().strip()
    actual_sig = hmac.new(
        CHECKPOINT_SECRET.encode(), data, hashlib.sha256
    ).hexdigest()
    if not hmac.compare_digest(actual_sig, expected_sig):
        raise ValueError("Checkpoint integrity check failed - file may be tampered")
    import pickle
    return pickle.loads(data)
```

注意：方案 B 需要在保存 checkpoint 时同步生成签名文件，且 `_CHECKPOINT_SECRET` 必须由 Rust 侧生成并注入（不能硬编码）。

**建议先用方案 A，逐步替换 pickle 使用**。

- [ ] **Step 3.3: 编译/运行验证**

```bash
cd src-tauri && cargo test python -- --nocapture 2>&1 | tail -20
```

- [ ] **Step 3.4: Commit**

```bash
git add src-tauri/src/python/_restore.py
git commit -m "fix(python): replace pickle deserialization in checkpoint restore

pickle.load on untrusted files is a remote code execution vector.
Replace with JSON-based checkpoint format to eliminate the attack
surface.

Ref: P0/PY3"
```

---

## Task 4：S1 — `AgentRuntime::for_test()` 用于生产初始化

**Problem:** `lib.rs:110` 使用 `AgentRuntime::for_test()`，内部是 `InMemoryAgentInvocationStore`。进程重启后所有子 agent 调用记录丢失。目前不存在 `FileAgentInvocationStore`。

**Files:**
- Create: `src-tauri/src/runtime/agent/file_agent_invocation_store.rs`
- Modify: `src-tauri/src/runtime/agent/mod.rs` (导出新 store)
- Modify: `src-tauri/src/runtime/agent/agent_runtime.rs` (加 `from_storage` 构造函数)
- Modify: `src-tauri/src/lib.rs` (第 110 行，改用持久化实现)

- [ ] **Step 4.1: 读取当前 AgentInvocationStore trait 定义**

```bash
grep -n "AgentInvocationStore\|InvocationRecord\|list_invocations\|update_invocation" \
  src-tauri/src/runtime/agent/agent_runtime.rs | head -30
```

- [ ] **Step 4.2: 创建 FileAgentInvocationStore**

```rust
// src-tauri/src/runtime/agent/file_agent_invocation_store.rs
use std::path::PathBuf;
use std::sync::Mutex;
use anyhow::Result;
use super::agent_runtime::{AgentInvocationStore, AgentInvocationRecord, AgentStatus};

pub struct FileAgentInvocationStore {
    store_path: PathBuf,
    cache: Mutex<Vec<AgentInvocationRecord>>,
}

impl FileAgentInvocationStore {
    pub fn new(store_path: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(store_path.parent().unwrap_or(&store_path))?;
        let cache = if store_path.exists() {
            let data = std::fs::read_to_string(&store_path)?;
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(Self { store_path, cache: Mutex::new(cache) })
    }

    fn persist(&self, records: &[AgentInvocationRecord]) -> Result<()> {
        let data = serde_json::to_string_pretty(records)?;
        std::fs::write(&self.store_path, data)?;
        Ok(())
    }
}

impl AgentInvocationStore for FileAgentInvocationStore {
    fn insert_invocation(&self, record: AgentInvocationRecord) -> Result<()> {
        let mut cache = self.cache.lock().unwrap();
        cache.push(record);
        self.persist(&cache)
    }

    fn update_invocation_status(&self, agent_id: &str, status: AgentStatus) -> Result<()> {
        let mut cache = self.cache.lock().unwrap();
        for r in cache.iter_mut() {
            if r.agent_id.as_str() == agent_id {
                r.status = status.clone();
            }
        }
        self.persist(&cache)
    }

    fn list_invocations(&self) -> Result<Vec<AgentInvocationRecord>> {
        Ok(self.cache.lock().unwrap().clone())
    }
}
```

实际字段名根据 `AgentInvocationRecord` 的真实定义调整。

- [ ] **Step 4.3: 在 AgentRuntime 加 from_storage 构造函数**

```rust
// agent_runtime.rs 中加入：
impl AgentRuntime {
    pub fn from_storage(store_path: std::path::PathBuf) -> anyhow::Result<Self> {
        let store = crate::runtime::agent::file_agent_invocation_store::FileAgentInvocationStore::new(store_path)?;
        Ok(Self { store: std::sync::Arc::new(store) })
    }
}
```

- [ ] **Step 4.4: 修改 lib.rs 改用持久化实现**

```rust
// lib.rs 第 110 行附近
// 改为：
let agent_store_path = app_data_dir.join("agent_invocations.json");
let agent_runtime = Arc::new(
    runtime::agent::AgentRuntime::from_storage(agent_store_path)
        .unwrap_or_else(|e| {
            log::warn!("Failed to create FileAgentInvocationStore: {e}, falling back to in-memory");
            runtime::agent::AgentRuntime::for_test()
        })
);
```

- [ ] **Step 4.5: 编译验证**

```bash
cd src-tauri && cargo build 2>&1 | grep -E "^error"
```

- [ ] **Step 4.6: Commit**

```bash
git add src-tauri/src/runtime/agent/file_agent_invocation_store.rs \
        src-tauri/src/runtime/agent/mod.rs \
        src-tauri/src/runtime/agent/agent_runtime.rs \
        src-tauri/src/lib.rs
git commit -m "feat(agent): add FileAgentInvocationStore for production use

Replace AgentRuntime::for_test() (InMemoryAgentInvocationStore) in
production initialization with a file-backed store. Agent invocation
records now persist across restarts.

Ref: P0/S1"
```

---

## Task 5：WF2/S2 — `chat_adapter` 先于 `facade` 创建，`authorized_workspace_store` 静默 None

**Problem:** `lib.rs:222` 创建 `chat_adapter`，`lib.rs:256` 才注册 `facade`。`TauriChatCommandAdapter::new()` 中 `try_state::<Arc<RuntimeRepositoryFacade>>()` 因时序问题始终返回 `None`，`authorized_workspace_store` 静默失效。用户已授权目录但 workspace-first 能力无法激活。

**Files:**
- Modify: `src-tauri/src/lib.rs` (初始化顺序)
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs` (加 warn log)

- [ ] **Step 5.1: 读取 lib.rs 初始化区域**

```bash
sed -n '100,280p' src-tauri/src/lib.rs
```

- [ ] **Step 5.2: 调整初始化顺序**

将 `facade` 的创建移到 `chat_adapter` 创建之前：

```rust
// 在 chat_adapter 创建之前先创建并注册 facade：
let facade = Arc::new(RuntimeRepositoryFacade::from_storage(db.clone()));
app.manage(facade.clone());  // 先注册，让 try_state 可以找到

// 然后再创建 chat_adapter（此时 try_state 能拿到 facade）
let chat_adapter = TauriChatCommandAdapter::new(...);
```

注意检查 facade 的创建是否依赖 `db` 以外的其他参数，确保依赖满足。

- [ ] **Step 5.3: 在 chat.rs 中 try_state 失败时加 warn log**

```rust
// transport/tauri_commands/chat.rs TauriChatCommandAdapter::new() 中：
let authorized_workspace_store = if let Some(facade) = services.app.try_state::<Arc<RuntimeRepositoryFacade>>() {
    Some(facade.authorized_workspace_store())
} else {
    log::warn!(
        "[TauriChatCommandAdapter] RuntimeRepositoryFacade not registered yet. \
         authorized_workspace_store will be None. Check initialization order in lib.rs."
    );
    None
};
```

- [ ] **Step 5.4: 编译验证**

```bash
cd src-tauri && cargo build 2>&1 | grep -E "^error"
```

- [ ] **Step 5.5: 验证修复（运行 workspace-first 相关测试）**

```bash
cd src-tauri && cargo test workspace_first -- --nocapture 2>&1 | tail -20
```

- [ ] **Step 5.6: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/transport/tauri_commands/chat.rs
git commit -m "fix(wiring): move facade registration before chat_adapter creation

authorized_workspace_store was silently None because RuntimeRepositoryFacade
was managed after TauriChatCommandAdapter::new() called try_state. Fix
initialization order and add warn log for future detection.

Ref: P0/WF2/S2"
```

---

## 完成检查

所有 5 个 Task 完成后：

- [ ] 运行 review_ 全量回归

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -30
```

- [ ] 运行前端测试

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
pnpm exec vitest run src/lib/tauri.events.test.ts \
  src/hooks/useStreaming.integration.test.tsx \
  src/stores/chatStore.test.ts
```

- [ ] 更新 gap-assessment.md 中 P0 项的状态为 ✅
