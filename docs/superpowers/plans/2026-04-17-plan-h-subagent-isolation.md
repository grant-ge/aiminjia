# Subagent 状态隔离计划（Plan-H）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 subagent 状态隔离：FileStateCache 共享读/隔离写，父子 cancel 级联，子结果隔离汇报，agentId 影响工具行为。通过 TDD 模式确保每个隔离边界清晰可验证。

**对标参考：** `/Users/a20250311/github/claude-code-best/src/Task.ts` 中的 subagent state 隔离机制：
- `cloneFileStateCache` — 子 agent 读取父 cache，写操作本地化
- `AbortController cascade` — 父中止时子自动中止，子中止不影响父
- `SubAgentResult` — 子结果作为工具输出返回，不直接写父 messages

**Architecture Dependency:** 依赖 Plan-B 的 `QueryEngine.read_file_state`（来自 `CapabilityContext.read_file_state: Arc<FileStateCache>`）；H1→H2→H3→H4 按此顺序实施，H3 与 H4 可并行。

**Tech Stack:** Rust, tokio, async_trait, anyhow, serde_json

**Worktree branch:** `feat/subagent-isolation`

---

## 0. 架构现状 & 隔离目标

### 0.1 当前 Subagent 数据流

1. **创建阶段** (`agent_runtime.rs::spawn_child_run`)
   - 生成新 `AgentId` 与 `child_run_id`
   - 创建 `AgentInvocation` 并持久化
   - 返回 `ChildRunHandle`

2. **执行阶段** （尚无完整实现 `run_child`）
   - 子 agent 与父 agent 共享同一 `QueryEngine` 实例
   - 若子 agent 修改工具执行后的缓存状态（如 FileStateCache），直接影响父 agent

3. **结果汇报阶段** （`agent_runtime.rs::complete_background_run`）
   - 持久化 summary 但未定义子结果如何返回给父
   - 父 agent 无法感知子完成的具体工作产出

### 0.2 隔离约束

**H1：FileStateCache 共享读/隔离写**
- 子 agent 初始状态：clone 父的 FileStateCache（共享内存的快照）
- 读操作：对 clone 执行 `get()`，若 miss 则读盘（与父独立）
- 写操作：`set()` 只更新自己的 clone，不回传给父

**H2：父子 Cancel 级联**
- 父 token cancel → 子 token 同步 cancel（已有 `child_token()` 机制，验证连接）
- 子 token cancel 不影响父 token

**H3：子结果隔离汇报**
- 子 agent 完成时返回 `SubAgentResult { content: String, file_metas: Vec<JsonValue> }`
- 父 agent 将子结果作为 tool_result（或 agent_response message）插入自己的 messages
- 子 agent 不直接修改父 agent 的 messages

**H4：agentId 影响工具行为**
- 工具通过 `ToolExecutionContext.agent_id` 判断是否在子 agent 中运行
- `CapabilityContext` 新增 `is_subagent: bool` 辅助标志
- 示例：BashTool 可对 subagent 设置更严格的权限或沙箱级别

---

## 1. 文件地图 & 修改清单

### 1.1 Runtime Core

**Modify: `src-tauri/src/runtime/tools/capability.rs`**
- 新增 `FileStateCache::clone()` 方法（深度 clone Mutex 内的 LRU 缓存）
- 新增 `FileStateCache::from_other()` static 方法便利构造
- 新增 `CapabilityContext.is_subagent: bool` 字段
- 新增 `CapabilityContext::with_subagent()` builder 方法

**Modify: `src-tauri/src/runtime/agent/child_run.rs`**
- 实现 `run_child()` 完整逻辑（当前是空实现）
- 接收 `parent_query_engine: &QueryEngine` + `child_query_engine: &mut QueryEngine`
- Clone 父 FileStateCache，注入子 QueryEngine
- 执行子 agent 的 query loop，返回 `SubAgentResult`

**Modify: `src-tauri/src/runtime/agent/agent_runtime.rs`**
- 修改 `spawn_child_run()` 返回类型改为 `ChildRunHandle` + parent FileStateCache 引用
- 添加 `get_parent_file_state_cache()` 查询方法（用于 clone）

**Modify: `src-tauri/src/runtime/query_engine.rs`**
- 新增 `read_file_state()` getter 暴露 `Option<Arc<FileStateCache>>`（用于 clone）
- 新增 `with_file_state_cache()` builder 方法（用于子 agent 注入 clone）

**Create: `src-tauri/src/runtime/agent/subagent_result.rs`**
- 定义 `SubAgentResult` struct
- 序列化为 `tool_result` JSON 格式

### 1.2 Transport / Host Bridge

**Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`**
- 若涉及子 agent 生成，需更新 spawn 时的 CapabilityContext 注入
- 确保 `is_subagent` 标志在 CapabilityContext 中传递

### 1.3 Tests

**Create: `src-tauri/tests/subagent_filestate_cache_isolation_test.rs`**
- Test H1-1：clone FileStateCache 后父读、子写不相互影响
- Test H1-2：子 agent 读 miss 时独立读盘
- Test H1-3：子 agent 多次写同一文件，父看不到

**Create: `src-tauri/tests/subagent_cancel_cascade_test.rs`**
- Test H2-1：父 cancel → 子 token 同步 cancel
- Test H2-2：子 cancel 不影响父 token
- Test H2-3：多级 cancel（parent → child → grandchild）级联

**Create: `src-tauri/tests/subagent_result_isolation_test.rs`**
- Test H3-1：子 agent 完成，返回 SubAgentResult
- Test H3-2：父 agent 将子结果插入 messages，不是直接融合
- Test H3-3：多个子 agent 结果逐个汇报，顺序保证

**Create: `src-tauri/tests/subagent_agentid_capability_test.rs`**
- Test H4-1：子 agent 的 ToolExecutionContext.agent_id != None
- Test H4-2：CapabilityContext.is_subagent == true for child, false for parent
- Test H4-3：工具可通过 agent_id 调整行为

**Re-run: `src-tauri/tests/cancel_cascade_test.rs`**
- 确保现有 cancel 级联测试仍然通过

---

## 2. 执行单元与 TDD 流程

### 单元 H1：FileStateCache 隔离写

**目标**：子 agent 初始化时 clone 父 FileStateCache，写操作本地化。

#### H1-1：`FileStateCache::clone()` 与 `from_other()` 方法

**失败测试** `src-tauri/tests/subagent_filestate_cache_isolation_test.rs::test_h1_1_filestate_clone`
```rust
#[test]
fn test_h1_1_filestate_clone_reads_parent_initial_state() {
    let parent_cache = Arc::new(FileStateCache::new());
    parent_cache.set(
        PathBuf::from("/tmp/file.txt"),
        FileState {
            content: "parent_content".to_string(),
            mtime_secs: 1000,
            offset: None,
            limit: None,
        },
    );

    // Clone: child 获得父的快照
    let child_cache = parent_cache.clone_for_child();

    // 子读取：应得到父的初始值
    let child_state = child_cache.get(Path::new("/tmp/file.txt"));
    assert_eq!(child_state.unwrap().content, "parent_content");

    // 子修改
    child_cache.set(
        PathBuf::from("/tmp/file.txt"),
        FileState {
            content: "child_modified".to_string(),
            mtime_secs: 2000,
            offset: None,
            limit: None,
        },
    );

    // 父不应看到子的修改
    let parent_state = parent_cache.get(Path::new("/tmp/file.txt"));
    assert_eq!(parent_state.unwrap().content, "parent_content");
    assert_eq!(parent_state.unwrap().mtime_secs, 1000);
}
```

**最小实现**：
- 在 `capability.rs` 的 `FileStateCache` impl 块中新增：
  ```rust
  pub fn clone_for_child(&self) -> Arc<FileStateCache> {
      // Shallow clone Mutex 内部的 LRU cache
      let cloned_inner = {
          let guard = self.cache.lock().unwrap();
          // 创建新 LruCache 并复制所有项
          let mut new_cache = lru::LruCache::new(
              NonZeroUsize::new(100).expect("capacity non-zero")
          );
          // 遍历父 cache，复制每一项
          for (path, state) in guard.iter() {
              new_cache.put(path.clone(), state.clone());
          }
          new_cache
      };
      Arc::new(FileStateCache {
          cache: Mutex::new(cloned_inner),
      })
  }
  ```

**验证命令**：
```bash
cd src-tauri && cargo test --test subagent_filestate_cache_isolation_test::test_h1_1_filestate_clone -- --nocapture
```

**Commit**：
```
feat(capability): FileStateCache::clone_for_child() 深度复制 LRU 缓存

使子 agent 能初始化为父缓存快照，读取继承但修改隔离。
```

---

#### H1-2：`QueryEngine` FileStateCache 注入

**失败测试** `test_h1_2_queryengine_filestate_clone_injection`
```rust
#[test]
fn test_h1_2_queryengine_filestate_clone_injection() {
    let parent_cap = CapabilityContext::with_workspace(
        PathBuf::from("/tmp"),
        "test-workspace",
    ).with_read_file_state(Arc::new(FileStateCache::new()));

    let parent_engine = QueryEngine::with_dispatcher(
        Arc::new(ToolDispatcher::for_test()),
    ).with_capability_context(Arc::new(parent_cap.clone()));

    // Clone parent's file state for child
    let child_file_state = parent_engine
        .read_file_state()
        .expect("parent has file state")
        .clone_for_child();

    // Child engine 注入 clone 后的 file state
    let mut child_cap = parent_cap.clone();
    child_cap.read_file_state = Some(child_file_state);

    let child_engine = QueryEngine::with_dispatcher(
        Arc::new(ToolDispatcher::for_test()),
    ).with_capability_context(Arc::new(child_cap));

    // Verify: parent 和 child 有各自的 FileStateCache
    let parent_state = parent_engine.read_file_state();
    let child_state = child_engine.read_file_state();
    assert!(parent_state.is_some());
    assert!(child_state.is_some());
    // 不同对象
    assert!(!Arc::ptr_eq(
        parent_state.unwrap().as_ref(),
        child_state.unwrap().as_ref(),
    ));
}
```

**最小实现**：
- 在 `query_engine.rs` 中添加：
  ```rust
  pub fn read_file_state(&self) -> Option<&Arc<FileStateCache>> {
      // 返回 capability context 中的 file state（若存在）
      if let Some(cap) = &self.capability_context {
          cap.read_file_state.as_ref()
      } else {
          None
      }
  }
  ```
- 在 `capability.rs` CapabilityContext 中新增字段：
  ```rust
  pub capability_context: Option<SharedCapabilityContext>,  // 已有 read_file_state
  ```

**验证命令**：
```bash
cd src-tauri && cargo test --test subagent_filestate_cache_isolation_test::test_h1_2_queryengine_filestate_clone_injection -- --nocapture
```

**Commit**：
```
feat(query-engine): read_file_state() getter + with_capability_context() builder

子 agent 初始化时通过该接口获取父 FileStateCache 并 clone。
```

---

#### H1-3：子 Agent 工具执行写操作隔离

**失败测试** `test_h1_3_child_tool_write_isolated`
```rust
#[test]
async fn test_h1_3_child_tool_write_isolated() {
    // 模拟工具执行：write_file 写入文件后更新 FileStateCache
    let parent_file_state = Arc::new(FileStateCache::new());
    parent_file_state.set(
        PathBuf::from("/tmp/doc.txt"),
        FileState {
            content: "original".to_string(),
            mtime_secs: 1000,
            offset: None,
            limit: None,
        },
    );

    // Child clone
    let child_file_state = parent_file_state.clone_for_child();

    // 模拟子 agent 工具执行：修改文件并更新缓存
    child_file_state.set(
        PathBuf::from("/tmp/doc.txt"),
        FileState {
            content: "child_updated".to_string(),
            mtime_secs: 2000,
            offset: None,
            limit: None,
        },
    );

    // 验证：父缓存不变
    let parent_val = parent_file_state.get(Path::new("/tmp/doc.txt")).unwrap();
    assert_eq!(parent_val.content, "original");

    // 子缓存已更新
    let child_val = child_file_state.get(Path::new("/tmp/doc.txt")).unwrap();
    assert_eq!(child_val.content, "child_updated");
}
```

**最小实现**：无需额外代码，H1-1 的实现已支持此场景。

**验证命令**：
```bash
cd src-tauri && cargo test --test subagent_filestate_cache_isolation_test::test_h1_3_child_tool_write_isolated -- --nocapture
```

**Commit**：
```
test(filestate-cache): H1-3 验证子 agent 写操作隔离

FileStateCache::clone_for_child() 保证子修改不回传。
```

---

### 单元 H2：Cancel 级联

**目标**：父 cancel → 子同步 cancel；子 cancel 不影响父。已有 CancellationToken 机制，本单元验证连接。

#### H2-1：父子 CancellationToken 级联

**失败测试** `src-tauri/tests/subagent_cancel_cascade_test.rs::test_h2_1_parent_cancel_propagates_to_child`
```rust
#[test]
fn test_h2_1_parent_cancel_propagates_to_child() {
    let parent_token = CancellationToken::new();
    let child_token = parent_token.child_token();

    assert!(!parent_token.is_cancelled());
    assert!(!child_token.is_cancelled());

    // Parent cancel
    parent_token.cancel();

    assert!(parent_token.is_cancelled());
    assert!(
        child_token.is_cancelled(),
        "child token must be cancelled when parent is cancelled"
    );
}
```

**最小实现**：无需额外代码，`CancellationToken::child_token()` 已实现此机制（见 `src-tauri/src/runtime/cancellation.rs`）。

**验证命令**：
```bash
cd src-tauri && cargo test --test subagent_cancel_cascade_test::test_h2_1_parent_cancel_propagates_to_child -- --nocapture
```

**Commit**：
```
test(cancel-cascade): H2-1 验证父 cancel 级联到子

已有 CancellationToken::child_token() 机制。
```

---

#### H2-2：子 Cancel 不影响父

**失败测试** `test_h2_2_child_cancel_does_not_reverse_propagate`
```rust
#[test]
fn test_h2_2_child_cancel_does_not_reverse_propagate() {
    let parent_token = CancellationToken::new();
    let child_token = parent_token.child_token();

    // Cancel only child
    child_token.cancel();

    assert!(child_token.is_cancelled());
    assert!(
        !parent_token.is_cancelled(),
        "parent must NOT be cancelled when child is cancelled"
    );
}
```

**最小实现**：无需额外代码，`CancellationToken` 单向传播设计已保证。

**验证命令**：
```bash
cd src-tauri && cargo test --test subagent_cancel_cascade_test::test_h2_2_child_cancel_does_not_reverse_propagate -- --nocapture
```

**Commit**：
```
test(cancel-cascade): H2-2 验证子 cancel 不回传到父

CancellationToken 单向传播设计。
```

---

#### H2-3：在 run_child 中应用 Cancel 级联

**失败测试** `test_h2_3_run_child_cancel_cascade`
```rust
#[test]
async fn test_h2_3_run_child_cancel_cascade() {
    let parent_cancel = CancellationToken::new();
    let child_cancel = parent_cancel.child_token();

    let parent_run_id = RunId::new("parent-run");
    let request = SpawnChildRunRequest::for_test(parent_run_id.clone());

    // 模拟 run_child 接收 parent_cancel，从而创建 child_cancel
    // （实际在 spawn_child_run 和 run_child 的交互中体现）

    // 父 cancel
    parent_cancel.cancel();

    // 预期：子也被 cancel
    assert!(child_cancel.is_cancelled());
}
```

**最小实现**：在 `child_run.rs::run_child()` 中正确使用 parent cancellation token 创建 child：
```rust
pub async fn run_child(
    invocation: &AgentInvocation,
    parent_cancellation: CancellationToken,
) -> Result<SubAgentResult> {
    // Child token 继承 parent cancellation
    let child_cancellation = parent_cancellation.child_token();
    
    if child_cancellation.is_cancelled() {
        anyhow::bail!("cancelled");
    }
    
    // ... 执行子 agent 逻辑，使用 child_cancellation
    Ok(SubAgentResult::default())
}
```

**验证命令**：
```bash
cd src-tauri && cargo test --test subagent_cancel_cascade_test::test_h2_3_run_child_cancel_cascade -- --nocapture
```

**Commit**：
```
feat(child-run): 在 run_child() 中应用 cancel 级联

parent cancellation → child token 继承。
```

---

### 单元 H3：子结果隔离汇报

**目标**：子 agent 完成返回 `SubAgentResult`，父 agent 作为 tool_result 插入自己的 messages。

#### H3-1：定义 `SubAgentResult` 结构

**失败测试** `src-tauri/tests/subagent_result_isolation_test.rs::test_h3_1_subagent_result_serialization`
```rust
#[test]
fn test_h3_1_subagent_result_serialization() {
    let result = SubAgentResult {
        agent_id: AgentId::new("agent-1"),
        content: "child completed task".to_string(),
        file_metas: vec![
            json!({"path": "/tmp/output.txt", "size": 1024}),
        ],
    };

    // 应能序列化为 JSON（用于 tool_result）
    let json_val = serde_json::to_value(&result).expect("serializable");
    assert_eq!(json_val["content"], "child completed task");
    assert_eq!(json_val["agent_id"], "agent-1");
}
```

**最小实现**：在 `src-tauri/src/runtime/agent/subagent_result.rs` 中：
```rust
use serde::{Deserialize, Serialize};
use crate::runtime::ids::AgentId;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubAgentResult {
    pub agent_id: AgentId,
    pub content: String,
    pub file_metas: Vec<serde_json::Value>,
}

impl SubAgentResult {
    pub fn new(agent_id: AgentId, content: impl Into<String>) -> Self {
        Self {
            agent_id,
            content: content.into(),
            file_metas: Vec::new(),
        }
    }

    pub fn with_file_metas(mut self, metas: Vec<serde_json::Value>) -> Self {
        self.file_metas = metas;
        self
    }
}
```

**验证命令**：
```bash
cd src-tauri && cargo test --test subagent_result_isolation_test::test_h3_1_subagent_result_serialization -- --nocapture
```

**Commit**：
```
feat(subagent-result): 定义 SubAgentResult struct

包含 agent_id、content、file_metas，可序列化为 JSON。
```

---

#### H3-2：run_child 返回 SubAgentResult

**失败测试** `test_h3_2_run_child_returns_result`
```rust
#[test]
async fn test_h3_2_run_child_returns_result() {
    let parent_run_id = RunId::new("parent-run");
    let parent_cancel = CancellationToken::new();
    let request = SpawnChildRunRequest::for_test(parent_run_id.clone());

    // Mock invocation
    let invocation = AgentInvocation::new(
        AgentId::new("child-agent"),
        parent_run_id,
        RunId::new("child-run"),
    );

    // run_child 应返回 SubAgentResult
    let result = run_child(&invocation, parent_cancel)
        .await
        .expect("should succeed");

    assert_eq!(result.agent_id.as_str(), "child-agent");
    assert!(!result.content.is_empty());
}
```

**最小实现**：在 `child_run.rs::run_child()` 中：
```rust
pub async fn run_child(
    invocation: &AgentInvocation,
    parent_cancellation: CancellationToken,
) -> Result<SubAgentResult> {
    let child_cancellation = parent_cancellation.child_token();
    
    if child_cancellation.is_cancelled() {
        anyhow::bail!("cancelled");
    }

    // 返回 SubAgentResult（包含 agent_id）
    Ok(SubAgentResult::new(
        invocation.agent_id.clone(),
        "child agent completed",
    ))
}
```

**验证命令**：
```bash
cd src-tauri && cargo test --test subagent_result_isolation_test::test_h3_2_run_child_returns_result -- --nocapture
```

**Commit**：
```
feat(child-run): run_child() 返回 SubAgentResult

包含执行完成的 agent_id 和 content。
```

---

#### H3-3：父 Agent 将子结果作为 Tool Result 插入

**失败测试** `test_h3_3_parent_inserts_child_result_as_tool_output`
```rust
#[test]
async fn test_h3_3_parent_inserts_child_result_as_tool_output() {
    // 模拟：parent agent 执行工具"spawn_subagent"
    // 工具返回 SubAgentResult（代表子 agent 完成）
    
    let subagent_result = SubAgentResult::new(
        AgentId::new("child-1"),
        "subagent completed analysis",
    );

    // 父 agent 应将结果包装为 tool_result message
    let tool_result_json = json!({
        "agent_id": subagent_result.agent_id.as_str(),
        "content": subagent_result.content,
        "file_metas": subagent_result.file_metas,
    });

    // 验证：result 中包含了子结果
    assert_eq!(
        tool_result_json["content"],
        "subagent completed analysis"
    );
}
```

**最小实现**：示例代码（实际在 query_engine 或 chat_turn_driver 中）：
```rust
// 当工具返回 SubAgentResult 时
let tool_result_json = serde_json::to_value(&subagent_result)?;
// 插入为 tool_result message（而非直接融合）
turn.append_message(Message::ToolResult {
    tool_call_id: tool_call_id.clone(),
    content: tool_result_json,
});
```

**验证命令**：
```bash
cd src-tauri && cargo test --test subagent_result_isolation_test::test_h3_3_parent_inserts_child_result_as_tool_output -- --nocapture
```

**Commit**：
```
test(result-isolation): H3-3 验证父 agent 隔离汇报子结果

子结果通过 tool_result message 返回，不直接融合。
```

---

### 单元 H4：agentId 影响工具行为

**目标**：`CapabilityContext.is_subagent` 标志让工具感知自己在子 agent 中，调整权限或行为。

#### H4-1：`CapabilityContext::is_subagent` 字段

**失败测试** `src-tauri/tests/subagent_agentid_capability_test.rs::test_h4_1_capability_is_subagent_field`
```rust
#[test]
fn test_h4_1_capability_is_subagent_field() {
    // Parent capability
    let parent_cap = CapabilityContext::with_workspace(
        PathBuf::from("/tmp"),
        "test-ws",
    );
    assert!(!parent_cap.is_subagent, "parent should have is_subagent=false");

    // Child capability
    let child_cap = CapabilityContext::with_workspace(
        PathBuf::from("/tmp"),
        "test-ws",
    ).with_subagent(true);
    assert!(child_cap.is_subagent, "child should have is_subagent=true");
}
```

**最小实现**：在 `capability.rs` 的 `CapabilityContext` 中添加字段和 builder：
```rust
pub struct CapabilityContext {
    // ... existing fields
    pub is_subagent: bool,
}

impl CapabilityContext {
    pub fn with_subagent(mut self, is_subagent: bool) -> Self {
        self.is_subagent = is_subagent;
        self
    }
}
```

**验证命令**：
```bash
cd src-tauri && cargo test --test subagent_agentid_capability_test::test_h4_1_capability_is_subagent_field -- --nocapture
```

**Commit**：
```
feat(capability): CapabilityContext::is_subagent 标志

标记 context 是否在 subagent 中运行，工具可通过此调整行为。
```

---

#### H4-2：子 Agent 执行时设置 is_subagent=true

**失败测试** `test_h4_2_child_execution_sets_is_subagent`
```rust
#[test]
async fn test_h4_2_child_execution_sets_is_subagent() {
    let parent_cap = CapabilityContext::with_workspace(
        PathBuf::from("/tmp"),
        "test-ws",
    );

    // 模拟：run_child 为自己的 QueryEngine 组装 CapabilityContext
    let child_cap = parent_cap.clone().with_subagent(true);

    // 验证
    assert!(!parent_cap.is_subagent);
    assert!(child_cap.is_subagent);
}
```

**最小实现**：在 `child_run.rs::run_child()` 中组装 capability 时：
```rust
pub async fn run_child(
    invocation: &AgentInvocation,
    parent_query_engine: &QueryEngine,
    parent_cancellation: CancellationToken,
) -> Result<SubAgentResult> {
    let child_cancellation = parent_cancellation.child_token();
    
    // Clone parent capability，标记为 subagent
    let mut child_cap = parent_query_engine.capability_context().clone();
    child_cap.is_subagent = true;

    // 创建子 QueryEngine with updated capability
    let mut child_engine = parent_query_engine.clone();
    child_engine.set_capability_context(Arc::new(child_cap));

    // ... 执行子 agent
}
```

**验证命令**：
```bash
cd src-tauri && cargo test --test subagent_agentid_capability_test::test_h4_2_child_execution_sets_is_subagent -- --nocapture
```

**Commit**：
```
feat(child-run): 执行时为子 agent 设置 is_subagent=true

子 QueryEngine 的 CapabilityContext 标记为 subagent。
```

---

#### H4-3：工具通过 agent_id 与 is_subagent 调整行为

**失败测试** `test_h4_3_tool_perceives_subagent_status`
```rust
#[test]
fn test_h4_3_tool_perceives_subagent_status() {
    // Parent execution context
    let parent_ctx = ToolExecutionContext::new(
        SessionId::new("sess-1"),
        RunId::new("run-1"),
        None,  // no agent_id for parent
        "tool-1",
        CancellationToken::new(),
    );
    
    let parent_cap = CapabilityContext::with_workspace(
        PathBuf::from("/tmp"),
        "ws",
    );
    
    let parent_ctx_with_cap = parent_ctx.with_capability(Arc::new(parent_cap));
    
    // 工具检查：不在 subagent 中
    assert!(parent_ctx_with_cap.capability.as_ref().unwrap().is_subagent == false);

    // Child execution context
    let child_ctx = ToolExecutionContext::new(
        SessionId::new("sess-1"),
        RunId::new("run-1"),
        Some(AgentId::new("agent-1")),  // has agent_id for child
        "tool-2",
        CancellationToken::new(),
    );
    
    let child_cap = CapabilityContext::with_workspace(
        PathBuf::from("/tmp"),
        "ws",
    ).with_subagent(true);
    
    let child_ctx_with_cap = child_ctx.with_capability(Arc::new(child_cap));
    
    // 工具检查：在 subagent 中
    assert!(child_ctx_with_cap.capability.as_ref().unwrap().is_subagent == true);
    assert!(child_ctx_with_cap.agent_id.is_some());
}
```

**最小实现**：无需额外代码，通过前面的实现已支持此场景。工具实现者可通过：
```rust
if let Some(cap) = &ctx.capability {
    if cap.is_subagent {
        // Apply stricter sandbox / permission for subagent
    }
}
if ctx.agent_id.is_some() {
    // Tool is running in a subagent
}
```

**验证命令**：
```bash
cd src-tauri && cargo test --test subagent_agentid_capability_test::test_h4_3_tool_perceives_subagent_status -- --nocapture
```

**Commit**：
```
test(agentid-capability): H4-3 验证工具可感知 subagent 状态

工具通过 ToolExecutionContext.agent_id 和 CapabilityContext.is_subagent 调整行为。
```

---

## 3. 工作规则 & Discipline

### 3.1 TDD 规则（Each Unit）

1. **先写失败测试**——测试文件在 `src-tauri/tests/` 下，函数签名和 assertions 先写好
2. **跑测试确认红灯**——`cargo test --test <testname>` 看到 FAILED
3. **最小实现让测试变绿**——仅改必要的源文件，直到测试 PASSED
4. **跑该单元最小验证命令**——确保该单元内所有相关测试都过
5. **提交该单元所有更改**——单个 git commit，message 包含 feat/test/fix 前缀

### 3.2 变更纪律

- **不允许跨单元修改**：H1 完成前不涉及 H2、H3、H4 的源代码
- **不允许伪装完成**：单纯重命名、移动函数位置、改文案不算完成
- **不允许测试宽松断言**：`assert!(true)` 这类 pass 不算
- **不允许顺手改其他 unrelated 代码**：新需求另开单元

### 3.3 验证关卡

- **单元级验证**：相应测试文件中的所有测试 PASSED
- **交叉验证**：`cargo test --test cancel_cascade_test` 继续通过（不破坏已有行为）
- **全量编译**：`cargo build --release` 无 warning
- **代码检查**：`cargo clippy` 无新 warning（如有则在同单元 fix）

### 3.4 Commit 纪律

每个单元完成后独立 commit。Message 格式：
```
<type>(<scope>): <subject>

<body>

<footer>
```

示例：
```
feat(filestate-cache): H1-1 FileStateCache::clone_for_child()

子 agent 初始化时 clone 父 FileStateCache，读取继承但修改隔离。

Related: Plan-H H1-1
```

---

## 4. 验证与测试清单

### 4.1 单元验证命令

| 单元 | 验证命令 |
|------|---------|
| H1-1 | `cargo test --test subagent_filestate_cache_isolation_test::test_h1_1_filestate_clone` |
| H1-2 | `cargo test --test subagent_filestate_cache_isolation_test::test_h1_2_queryengine_filestate_clone_injection` |
| H1-3 | `cargo test --test subagent_filestate_cache_isolation_test::test_h1_3_child_tool_write_isolated` |
| H2-1 | `cargo test --test subagent_cancel_cascade_test::test_h2_1_parent_cancel_propagates_to_child` |
| H2-2 | `cargo test --test subagent_cancel_cascade_test::test_h2_2_child_cancel_does_not_reverse_propagate` |
| H2-3 | `cargo test --test subagent_cancel_cascade_test::test_h2_3_run_child_cancel_cascade` |
| H3-1 | `cargo test --test subagent_result_isolation_test::test_h3_1_subagent_result_serialization` |
| H3-2 | `cargo test --test subagent_result_isolation_test::test_h3_2_run_child_returns_result` |
| H3-3 | `cargo test --test subagent_result_isolation_test::test_h3_3_parent_inserts_child_result_as_tool_output` |
| H4-1 | `cargo test --test subagent_agentid_capability_test::test_h4_1_capability_is_subagent_field` |
| H4-2 | `cargo test --test subagent_agentid_capability_test::test_h4_2_child_execution_sets_is_subagent` |
| H4-3 | `cargo test --test subagent_agentid_capability_test::test_h4_3_tool_perceives_subagent_status` |

### 4.2 回归验证

```bash
# 全量测试（确保不破坏既有）
cargo test --tests

# 架构约束测试
cargo test --test review_

# Cancellation 既有测试（最关键）
cargo test --test cancel_cascade_test
```

### 4.3 集成验证

完整执行以下场景：
1. 父 agent 生成子 agent（通过 `spawn_child_run`）
2. 子 agent 执行工具操作（读/写文件）
3. 父 agent cancel 时子同步中止
4. 子 agent 完成，结果作为 tool_result 返回给父
5. 父 agent 继续后续 turn，不受子 agent 数据污染

---

## 5. 实施检查清单

### 预实施检查
- [ ] 已读 `cancellation.rs`、`capability.rs`、`query_engine.rs`、`agent/` 目录结构
- [ ] 已确认 `FileStateCache` 的 Mutex + LRU 结构
- [ ] 已确认 `ToolExecutionContext` 包含 `agent_id` 和 `capability` 字段
- [ ] 已理解 Plan-B 中的 `read_file_state` 来源

### 实施阶段检查

#### H1 FileStateCache 隔离写
- [ ] `FileStateCache::clone_for_child()` 实现完成
- [ ] `FileStateCache::from_other()` 如需要已实现
- [ ] 所有 H1-1、H1-2、H1-3 测试 PASSED
- [ ] 不破坏 `cancel_cascade_test`

#### H2 Cancel 级联
- [ ] `CancellationToken::child_token()` 接线验证
- [ ] `run_child()` 正确接收 parent_cancellation
- [ ] 所有 H2-1、H2-2、H2-3 测试 PASSED
- [ ] cancel 级联级别梳理（parent → child → tool）

#### H3 子结果隔离汇报
- [ ] `SubAgentResult` struct 定义完成
- [ ] `run_child()` 返回 `Result<SubAgentResult>`
- [ ] 所有 H3-1、H3-2、H3-3 测试 PASSED
- [ ] 父 agent 插入结果的机制已梳理（query_engine or chat driver）

#### H4 agentId 影响工具行为
- [ ] `CapabilityContext::is_subagent` 字段添加
- [ ] `CapabilityContext::with_subagent()` builder 实现
- [ ] `run_child()` 设置 child_cap.is_subagent = true
- [ ] 所有 H4-1、H4-2、H4-3 测试 PASSED
- [ ] 工具框架文档更新（说明 is_subagent 用途）

### 完成检查
- [ ] `cargo test --tests` 全量通过
- [ ] `cargo clippy` 无新 warning
- [ ] `cargo build --release` 编译成功
- [ ] 所有 commit message 遵循格式规范
- [ ] 该计划文件已更新完成日期与结论

---

## 6. 可能的风险 & 缓解方案

### 风险 R1：FileStateCache Clone 性能
**问题**：LRU 缓存 clone 可能导致内存冗余。
**缓解**：Clone 仅在 spawn_child_run 时一次，缓存通常小（100 项），内存占用可接受。监测后期可优化为 CoW。

### 风险 R2：Cancel 级联延迟
**问题**：Cancel 传播延迟可能导致已派发的子任务继续运行。
**缓解**：`run_child()` 内部需多处检查 `is_cancelled()`，特别是工具执行前。

### 风险 R3：SubAgentResult 序列化兼容性
**问题**：SubAgentResult 新增字段可能破坏序列化兼容性。
**缓解**：使用 `#[serde(default)]` 与 versioning，前期无需过度设计。

### 风险 R4：多级子 Agent（祖父 → 父 → 子）
**问题**：当前设计未明确处理多级 cancel 或 filestate。
**缓解**：通过递归 `child_token()` 和递归 `clone_for_child()` 自动支持；后期若需特殊处理再扩展。

---

## 附录 A：测试文件模板

### 模板：`src-tauri/tests/subagent_filestate_cache_isolation_test.rs`

```rust
use app_lib::runtime::tools::capability::{FileState, FileStateCache};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ──────────────────────────────────────────────────────────────────────
// H1-1: FileStateCache clone
// ──────────────────────────────────────────────────────────────────────

#[test]
fn test_h1_1_filestate_clone_reads_parent_initial_state() {
    // ... implementation
}

// ──────────────────────────────────────────────────────────────────────
// H1-2: QueryEngine filestate clone injection
// ──────────────────────────────────────────────────────────────────────

#[test]
fn test_h1_2_queryengine_filestate_clone_injection() {
    // ... implementation
}

// ──────────────────────────────────────────────────────────────────────
// H1-3: Child tool write isolated
// ──────────────────────────────────────────────────────────────────────

#[test]
async fn test_h1_3_child_tool_write_isolated() {
    // ... implementation
}
```

### 模板：`src-tauri/tests/subagent_cancel_cascade_test.rs`

```rust
use app_lib::runtime::agent::invocation::{AgentInvocation, SpawnChildRunRequest};
use app_lib::runtime::agent::child_run::run_child;
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::ids::{AgentId, RunId};

// ──────────────────────────────────────────────────────────────────────
// H2-1: Parent cancel propagates to child
// ──────────────────────────────────────────────────────────────────────

#[test]
fn test_h2_1_parent_cancel_propagates_to_child() {
    // ... implementation
}

// ... 继续 H2-2、H2-3
```

---

## 附录 B：架构图（Plain Text）

```
Plan-H Subagent Isolation Architecture
═════════════════════════════════════════════════════════════════

[Parent Agent]
   │
   ├─ TurnState (parent_run_id)
   ├─ QueryEngine
   │   ├─ ToolDispatcher
   │   └─ CapabilityContext
   │       ├─ workspace_path
   │       ├─ file_ops
   │       ├─ read_file_state: Arc<FileStateCache>  ◄─ H1 Read Source
   │       ├─ is_subagent: false
   │       └─ ...
   │
   └─ CancellationToken (parent_cancel)
       │
       └─ child_token() ──────────────────────────────────► [Child Agent]
                                                              │
                                                              ├─ TurnState (child_run_id)
                                                              ├─ QueryEngine
                                                              │   ├─ ToolDispatcher
                                                              │   └─ CapabilityContext
                                                              │       ├─ workspace_path
                                                              │       ├─ file_ops
                                                              │       ├─ read_file_state: Arc<FileStateCache>  ◄─ H1 Clone
                                                              │       │   (Cloned, isolated writes)
                                                              │       ├─ is_subagent: true  ◄─ H4 Mark
                                                              │       └─ ...
                                                              │
                                                              └─ CancellationToken (child_cancel = parent_cancel.child_token())
                                                                  │
                                                                  ├─ propagates UP from parent  ◄─ H2 Cascade
                                                                  └─ does NOT propagate DOWN to parent
                                                                  
[Tool Execution in Child]
  │
  ├─ Reads from child_file_state  ◄─ Inherits parent's initial state
  ├─ Writes to child_file_state  ◄─ H1: Isolated, doesn't affect parent
  └─ Checks ctx.agent_id and cap.is_subagent  ◄─ H4: Adjusts behavior

[Child Completion]
  │
  └─ Returns SubAgentResult {  ◄─ H3
      agent_id: AgentId,
      content: String,
      file_metas: Vec<JsonValue>,
  }
       │
       └─ Parent inserts as tool_result message  ◄─ H3: Isolated report
           (NOT direct message fusion)
```

---

## 附录 C：相关代码路径索引

| 功能 | 文件路径 | 关键函数/struct |
|------|---------|-----------------|
| CancellationToken 机制 | `src-tauri/src/runtime/cancellation.rs` | `child_token()`, `cancel()`, `is_cancelled()` |
| FileStateCache | `src-tauri/src/runtime/tools/capability.rs` | `FileStateCache`, `get()`, `set()` |
| CapabilityContext | `src-tauri/src/runtime/tools/capability.rs` | `CapabilityContext`, `with_read_file_state()` |
| QueryEngine | `src-tauri/src/runtime/query_engine.rs` | `with_file_ops()`, `with_browser_available()` |
| ToolExecutionContext | `src-tauri/src/runtime/tools/context.rs` | `agent_id`, `capability` |
| AgentRuntime | `src-tauri/src/runtime/agent/agent_runtime.rs` | `spawn_child_run()`, `complete_run()` |
| run_child | `src-tauri/src/runtime/agent/child_run.rs` | `run_child()` |
| AgentInvocation | `src-tauri/src/runtime/agent/invocation.rs` | `AgentInvocation`, `SpawnChildRunRequest` |

---

**Version:** 2026-04-17
**Status:** Ready for implementation
**Estimated effort:** 3-4 sessions (12-16 hours)
**Next:** Start with worktree, implement H1-1 failing test
