# rules.md — cancellation-e2e 取消语义端到端测试意图

来源：[LUT-6](mention://issue/61eb2d45-0626-4b6a-840e-0209133260d6)

涉及核心模块：`runtime/cancellation.rs`（`CancellationToken` / `CancellationReason`）、`runtime/agent/cancellation_registry.rs`（`CancellationRegistry`）

`CancellationReason` 变体：`UserCancel`、`Interrupt`、`SiblingError`、`BackgroundStop`

---

## 意图 1：parent cancel 传播到 child，child cancel 不传播到 parent

**场景**
`child_token()` 建立的父子关系：父取消后子应立即被取消；子取消不影响父。这是级联取消的基础契约。

**前提**
- 构造 `parent = CancellationToken::new()`
- 调用 `child = parent.child_token()`

**操作**
1. 调用 `parent.cancel_with_reason(CancellationReason::UserCancel)`
2. 检查 `parent.is_cancelled()` 和 `child.is_cancelled()`
3. 检查 `parent.reason()` 和 `child.reason()`

**断言**
- `parent.is_cancelled() == true`
- `child.is_cancelled() == true`
- `parent.reason() == Some(CancellationReason::UserCancel)`
- `child.reason() == Some(CancellationReason::UserCancel)`

---

## 意图 2：child cancel 不传播到 parent，parent reason 保持 None

**场景**
子代理失败不应向上污染父代理的取消状态。

**前提**
- 构造 `parent = CancellationToken::new()`
- `child = parent.child_token()`

**操作**
1. 调用 `child.cancel_with_reason(CancellationReason::SiblingError)`

**断言**
- `child.is_cancelled() == true`
- `child.reason() == Some(CancellationReason::SiblingError)`
- `parent.is_cancelled() == false`
- `parent.reason() == None`

---

## 意图 3：三层嵌套 parent cancel 传播到 grandchild

**场景**
多层子代理嵌套时，顶层取消应级联到所有子孙代理。

**前提**
- 构造 `parent = CancellationToken::new()`
- `child = parent.child_token()`
- `grandchild = child.child_token()`

**操作**
1. 调用 `parent.cancel_with_reason(CancellationReason::Interrupt)`

**断言**
- `child.is_cancelled() == true`
- `grandchild.is_cancelled() == true`
- `child.reason() == Some(CancellationReason::Interrupt)`
- `grandchild.reason() == Some(CancellationReason::Interrupt)`

---

## 意图 4：从已取消的 parent 创建 child，child 立即处于 cancelled 状态

**场景**
在 parent 已取消后才创建的子代理，应立即继承取消状态，不进入执行。

**前提**
- 构造 `parent = CancellationToken::new()`
- 调用 `parent.cancel_with_reason(CancellationReason::Interrupt)`

**操作**
1. 调用 `child = parent.child_token()`

**断言**
- `child.is_cancelled() == true`
- `child.reason() == Some(CancellationReason::Interrupt)`

---

## 意图 5：child_token_ignoring_reason 忽略指定 reason，不传播该 reason 给 child

**场景**
某些子代理需要忽略特定的取消原因（如 SiblingError），继续执行，不被兄弟错误波及。

**前提**
- 构造 `parent = CancellationToken::new()`
- `child = parent.child_token_ignoring_reason(CancellationReason::SiblingError)`

**操作**
1. 调用 `parent.cancel_with_reason(CancellationReason::SiblingError)`

**断言**
- `parent.is_cancelled() == true`
- `child.is_cancelled() == false`（SiblingError 被忽略，child 未被取消）

---

## 意图 6：cancel 幂等，多次调用只有第一次生效

**场景**
用户多次点击停止，不应改变已设定的取消原因，也不应 panic。

**前提**
- 构造 `token = CancellationToken::new()`

**操作**
1. 调用 `token.cancel_with_reason(CancellationReason::UserCancel)`
2. 调用 `token.cancel_with_reason(CancellationReason::Interrupt)`（第二次，不同 reason）

**断言**
- `token.is_cancelled() == true`
- `token.reason() == Some(CancellationReason::UserCancel)`（第一次设定的 reason 保持不变）

---

## 意图 7：CancellationRegistry register 后 get 返回相同 token

**场景**
`CancellationRegistry` 按 `(session, team_name, agent_id)` 三维键存取 token，注册后能通过同样的键取回。

**前提**
- 构造 `registry = CancellationRegistry::new()`
- `session = SessionId::new("sess-1")`，`agent = AgentId::new("agent-1")`，`token = CancellationToken::new()`

**操作**
1. 调用 `registry.register(&session, "team-a", agent.clone(), token.clone()).await`
2. 调用 `registry.get(&session, "team-a", &agent).await`

**断言**
- 步骤 2 返回 `Some(retrieved_token)`
- 取消 `token` 后（`token.cancel()`），`retrieved_token.is_cancelled() == true`（两者指向同一 Arc）

---

## 意图 8：CancellationRegistry unregister 后 get 返回 None

**场景**
子代理退出后应从 registry 移除，避免泄漏和误操作。

**前提**
- 同意图 7 完成 register

**操作**
1. 调用 `registry.unregister(&session, "team-a", &agent).await`
2. 调用 `registry.get(&session, "team-a", &agent).await`

**断言**
- 步骤 2 返回 `None`

---

## 意图 9：cancel_team 取消 team 内所有 token 并清除注册

**场景**
取消整个 team 的所有子代理，registry 中该 team 的记录全部清除。

**前提**
- 构造 registry，同一 session 下同一 team-a 注册 2 个 token：`agent-x` 和 `agent-y`

**操作**
1. 调用 `registry.cancel_team(&session, "team-a").await`
2. 分别调用 `registry.get(&session, "team-a", &agent_x).await` 和 `get(&session, "team-a", &agent_y).await`

**断言**
- `cancel_team` 返回值等于 `2`（取消了 2 个 token）
- 两个 token 的 `is_cancelled() == true`
- 步骤 2 两次 get 均返回 `None`（已被移除）

---

## 意图 10：不同 team_name 的 token 互相隔离，cancel_team 只影响目标 team

**场景**
team 维度的隔离：取消 team-a 不应影响 team-b 的 token。

**前提**
- 同一 session，team-a 注册 `agent-1`，team-b 注册 `agent-2`

**操作**
1. 调用 `registry.cancel_team(&session, "team-a").await`
2. 调用 `registry.get(&session, "team-b", &agent_2).await`

**断言**
- `agent_1` 对应 token `is_cancelled() == true`
- 步骤 2 返回 `Some(token_b)`，且 `token_b.is_cancelled() == false`
