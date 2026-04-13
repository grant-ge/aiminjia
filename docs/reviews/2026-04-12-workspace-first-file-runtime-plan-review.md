# 2026-04-12 Workspace-First 文件能力模型方案 Review

## 状态

- 状态：**方案评审已关闭，可进入代码实现（v4 文档所有 P1/P2 已关闭）**
- 结论：14 个 findings（第一轮 8 个 + 第二轮 6 个）全部已修复，方案文档 v4 已收敛
- 修订文档：`docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md` v4
- 注意：代码实现阶段的回归 review 需单独建立 review 文档，本文档只覆盖方案评审

## 文档更新要求

后续这轮专项，`方案文档` 与 `review 文档` 应作为我和 Claude 的主要协作媒介，不再依赖聊天里的零散口头同步。

- Claude 先改方案文档：`/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md`
- 不要在当前 review 仍有 `P1/P2` 未关闭时直接进入代码实现
- Claude 每次修订方案后，需要同步回填这份 review 文档：
  - 把对应 finding 的状态从 `未修复` 改成 `已修复` 或 `部分修复`
  - 在 finding 下追加“修订说明 / 新增接线 / 新增测试 / 仍未解决项”
  - 如果修订过程中引入新的断层，需要在这份 review 文档继续追加 finding，而不是只在聊天里说
- 我下一轮验收默认以这两份文档为准：
  - 方案文档负责描述目标设计与实施步骤
  - review 文档负责记录 blocking issues、关闭状态与验收门槛
- 当这份 review 文档中的 `P1/P2` findings 全部关闭后，再进入代码实现 / TDD / 回归 review

## 本轮要求 Claude 输出什么

Claude 下一轮修订，至少要在方案文档中补齐并写实下面内容：

1. 真实 `send_message` 主链路里的授权目录注入接线
2. “有授权才暴露 workspace 工具 schema”的能力感知规则
3. analysis precompute 路径的授权目录接线
4. Python 对授权目录的只读 / 可写语义
5. 单 session 单当前目录的 store 覆盖语义
6. 所有工具名、上下文 contract、前端 IPC contract 的统一说法

## Review 范围

本轮 review 只针对方案文档与当前真实生产接线是否一致，不做代码实现修改。

- 方案文档：`/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md`
- 问题定义基线：`/Users/a20250311/IdeaProjects/lotus-app/docs/2026-04-12-runtime-gap-problem-statement.md`
- 真实生产链路参考：
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/plugin/registry.rs`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/plugin/context.rs`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/session_runtime.rs`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/tools/capability.rs`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/python/sandbox.rs`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/commands/workspace.rs`
  - `/Users/a20250311/IdeaProjects/lotus-app/src/lib/tauri.ts`

## Review 方法

本轮按“方案是否真正落到当前生产主链路”来审，不按文档自身是否自洽来审。

重点核对了 5 条链路：

1. `send_message -> chat_runtime_impl -> PluginContext -> ToolRegistry::execute`
2. `tool schema exposure -> ToolRegistry::get_schemas_filtered(ToolFilter::All)`
3. `analysis precompute -> SandboxConfig::for_workspace -> Python`
4. `authorized workspace store -> session truth source -> repeated authorization`
5. `frontend wrapper -> tauri command contract`

## 总体结论

当前方案的主要问题不是“方向不对”，而是“关键接线点还停留在理想 runtime 路径，没有完全贴住当前真实生产路径”。

如果 Claude 按这版文档直接开工，最容易出现的结果是：

- 文档级 golden path 看起来成立
- 局部单测也能写出来
- 但真实 `send_message`、analysis precompute、默认 daily 会话、前端 IPC 会各自留下一段断层

因此，这份方案需要先做一轮文档修订，再进入代码实现。

## 第二轮复审 Findings

### Finding 9 - 真实注入方案仍依赖 `chat_runtime_impl` 中不存在的 facade

- 状态：`已修复`
- 严重级别：`P1`
- 文档定位：`/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:232`
- 真实代码定位：
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:1`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat.rs:73`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/lib.rs:251`

#### 问题描述

文档已经把授权目录注入点改到 `chat_runtime_impl.rs`，方向是对的；但示意代码仍直接写成 `facade.authorized_workspace_store()`。当前真实 `legacy_send_message_impl(...)` 参数列表里没有 `RuntimeRepositoryFacade`，`TauriChatServices` 也没有这个字段。

这意味着关键接线仍然缺一跳：**到底是通过 `app.state::<Arc<RuntimeRepositoryFacade>>()` 读取，还是把 facade 从 `TauriChatCommandAdapter`/`TauriChatServices` 显式下传到 `legacy_send_message_impl()`。**

#### 为什么这是 blocking issue

如果这一步不写实，Claude 进入代码实现时仍然会碰到：

- 文档说要查 `authorized_workspace_store()`
- 但真实生产函数里根本拿不到 facade

这会直接让 Finding 1 的“真实接线”重新退回半空状态。

#### 文档修正要求

必须在方案里二选一并写死：

1. **显式下传方案**：给 `TauriChatServices` 增加 `facade: Arc<RuntimeRepositoryFacade>`，一路传到 `legacy_send_message_impl()`
2. **按 app.state 读取方案**：明确在 `chat_runtime_impl.rs` 用 `app.state::<Arc<RuntimeRepositoryFacade>>()` 取 managed state，并写出失败处理方式

同时，W2/W3 的代码示意和测试说明都要跟这个选定方案保持一致。

#### 修订说明

**修复方式：** 选定方案 2（`app.try_state`）。

`RuntimeRepositoryFacade` 已通过 `lib.rs:256` 注册为 `app.manage(facade)`，`legacy_send_message_impl` 收到 `app: AppHandle`，取法与 `agent_runtime`、`connector_engine` 完全一致：

```rust
app.try_state::<Arc<RuntimeRepositoryFacade>>()
    .and_then(|facade| facade.authorized_workspace_store()
        .get_current_for_session(...).ok().flatten())
```

不需要修改 `TauriChatServices` 或 `legacy_send_message_impl` 参数列表。plan §3.3 注入代码示意已全部改为 `app.try_state` 取法，`facade.xxx()` 裸调用已清除。

---

### Finding 10 - 文档仍残留旧的 `session_runtime` / 旧 store 路径叙述

- 状态：`已修复`
- 严重级别：`P2`
- 文档定位：
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:380`
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:811`
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:1093`
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:1141`
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:1146`

#### 问题描述

虽然文档前面已经明确“真实接线应落在 `chat_runtime_impl.rs`”，但后文仍残留不少旧说法：

- `4.0` 关键接线路径仍写授权目录经过 `session_runtime.rs`
- W2 文件清单仍把 `session_runtime.rs` 列为核心改动，却没把 `chat_runtime_impl.rs` 列进去
- 回归测试仍引用已拆掉的 `allowed_paths`
- golden path 仍写 `save(ws)` / `get_by_session()` / `session_runtime::execute_turn()`

#### 为什么这是问题

这会让实现者按两套路径同时改：

- 一套是新写法：`chat_runtime_impl + replace_for_session + allowed_read_paths`
- 一套是旧写法：`session_runtime + save/get_by_session + allowed_paths`

结果就是文档内部继续自相矛盾。

#### 文档修正要求

需要做一轮全文一致性清理：

- 关键接线路径图只保留真实生产路径
- 分期文件清单同步改成真实受影响文件
- golden path、TDD、回归示例全部替换成新 contract
- `allowed_paths` / `save` / `get_by_session` / `session_runtime::execute_turn()` 等旧表述全部清掉

#### 修订说明

**修复方式：**
- `4.0` 关键接线路径图已重写：移除 `session_runtime.rs` 路径，改为真实三处注入点（`:~1560` schema 感知、`:~1642` precompute、`:2596` PluginContext 构造）
- W2 文件清单中 `session_runtime.rs` 和 `child_run.rs` 已替换为 `chat_runtime_impl.rs`
- 回归测试和 W3 完成标志中 `allowed_paths` 已全部改为 `allowed_read_paths`/`allowed_write_paths`
- `save`/`get_by_session` 已全部改为 `replace_for_session`/`get_current_for_session`

---

### Finding 11 - schema 感知过滤的 TDD 仍然不能证明真实生产行为

- 状态：`已修复`
- 严重级别：`P1`
- 文档定位：`/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:983`

#### 问题描述

当前这条测试仍然只是：

- 在 registry 上注册工具
- 手工调用 `ToolFilter::Exclude(WORKSPACE_TOOL_NAMES)`
- 断言 workspace schema 被排除

它**没有经过真实 `chat_runtime_impl` 里的 `authorized_workspace_present` 判断**，也没有证明“按当前 session 是否有授权目录来决定发给 LLM 的 schema 列表”。

#### 为什么这是 blocking issue

就算生产代码里完全忘了写：

```rust
let authorized_workspace_present = ...
```

这条测试仍然会绿。它证明的是 `Exclude` API 好使，不是这次新增的真实行为好使。

#### 文档修正要求

必须把这条测试升级成“打到真实生产分支”的行为测试，例如：

- 抽一个可测试的 `build_visible_tool_defs(...)` helper，并在 `chat_runtime_impl` 真实调用它
- 或直接为 `chat_runtime_impl` 的 schema 构造逻辑补 targeted test

通过条件应变成：

- 有授权目录时，真实生产 helper 返回含 workspace 工具的 schema
- 无授权目录时，真实生产 helper 返回不含 workspace 工具的 schema

而不是继续只测 `ToolFilter::Exclude(...)`。

#### 修订说明

**修复方式：** 抽取 `build_visible_tool_defs(registry, has_authorized_workspace: bool) -> Vec<ToolDefinition>` 为独立 `pub(crate)` helper，放在 `chat_runtime_impl.rs`。主路径调用 `build_visible_tool_defs(&tool_registry, authorized_workspace_present).await`。

新增两条测试打到真实 helper：
- `test_build_visible_tool_defs_with_authorized_workspace` — 有授权时包含 workspace 工具
- `test_build_visible_tool_defs_without_authorized_workspace` — 无授权时排除全部 4 个 workspace 工具

原来只测 `ToolFilter::Exclude` API 的测试 `test_workspace_schema_excluded_when_no_authorized_workspace` 已从 plan 中删除。

---

### Finding 12 - precompute 的 TDD 仍然没有打到真实主链路

- 状态：`已修复`
- 严重级别：`P1`
- 文档定位：`/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:1047`
- 真实代码定位：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:1631`

#### 问题描述

`test_precompute_reads_authorized_directory` 现在只是手工 new：

```rust
SandboxConfig::for_workspace_with_authorized(...)
```

然后断言 read/write path 列表。这并没有经过 `chat_runtime_impl.rs:1642` 的真实 precompute 选择逻辑，也没有证明 analysis/workflow 模式下会因为 session 授权而真正走进这条分支。

#### 为什么这是 blocking issue

它仍然可能假绿：

- `for_workspace_with_authorized(...)` 实现正确
- 但 `chat_runtime_impl` 忘了在 precompute 场景调用它

这种情况下测试仍然通过，真实产品行为仍然错误。

#### 文档修正要求

必须把 precompute 测试升级成真实行为测试，例如：

- 抽一个 `build_precompute_sandbox(...)` helper，并让 `chat_runtime_impl` 实际调用它
- 测试直接驱动这个 helper，输入“有授权 / 无授权”的 session 上下文，断言返回的 sandbox 语义不同

至少要证明：

- 有授权目录时，precompute 走 `for_workspace_with_authorized`
- 无授权目录时，precompute 走 `for_workspace`

#### 修订说明

**修复方式：** 抽取 `build_precompute_sandbox(workspace_path: &PathBuf, authorized: Option<&AuthorizedWorkspaceRef>) -> SandboxConfig` 为独立 `pub(crate)` helper，放在 `chat_runtime_impl.rs`。主路径调用 `build_precompute_sandbox(&workspace_path, authorized_workspace.as_ref())`。

新增两条测试打到真实 helper：
- `test_build_precompute_sandbox_with_authorized_workspace` — 有授权时 `allowed_read_paths` 含授权目录，`allowed_write_paths` 不含
- `test_build_precompute_sandbox_without_authorized_workspace` — 无授权时只有旧 7 路径

原来只测 `SandboxConfig::for_workspace_with_authorized` 内部的集成测试已改为调用 `build_precompute_sandbox` helper。

---

### Finding 13 - 前端 IPC contract 仍然没有完全收敛

- 状态：`已修复`
- 严重级别：`P2`
- 文档定位：`/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:760`

#### 问题描述

文档已经删掉了 `listAuthorizedDirectory`，这是对的；但前端 contract 里仍残留两处未收敛：

- `revokeAuthorizedWorkspace(id: string)` 仍是按 `id` 撤销，而前面的 store / command 语义已经变成 `clear_for_session(session_id)`
- 还写着 `workspace:authorized` 事件，但文档没有定义任何后端发射点、payload 或监听方

#### 为什么这是问题

这会继续制造 UI/IPC 断层：

- 前端 wrapper 参数和后端命令参数不一致
- 幽灵事件只存在于类型声明里，不存在于真正协议里

#### 文档修正要求

必须把前端 contract 收敛成一套：

- `revokeAuthorizedWorkspace` 到底按 `sessionId` 调，还是先 `getAuthorizedWorkspace` 再用 `id`
- 如果后端命令按 session 清空，前端 wrapper 也必须改成 `revokeAuthorizedWorkspace(sessionId: string)`
- `workspace:authorized` 若不打算实现，就从方案文档删除；若要保留，就补完整事件合同

#### 修订说明

**修复方式：**
- `revokeAuthorizedWorkspace` 已改为接受 `sessionId: string`（与后端 `clear_for_session(session_id)` 对齐）
- `workspace:authorized` 事件已从 plan 中删除（UI 在用户触发授权后由前端同步更新状态，无需后端推送）
- plan §13（tauri.ts 改造内容）已同步更新

---

### Finding 14 - review 文档被过早标记为已关闭

- 状态：`已修复`
- 严重级别：`P2`
- 文档定位：`/Users/a20250311/IdeaProjects/lotus-app/docs/reviews/2026-04-12-workspace-first-file-runtime-plan-review.md:3`

#### 问题描述

这份 review 文档顶部已经写成：

- `已关闭（P1/P2 全部修复）`
- `可进入代码实现`

但当前方案文档仍然存在这轮复审新增的 6 个未闭合问题，而且 review 文档后文还保留了“当前方案仍需先修文档再进实现”的旧结论。

#### 为什么这是问题

review 文档现在已经不是可信的 gating 文档了。Claude 如果只看顶部状态，会以为可以直接实现；但如果往下看具体 findings，又仍然是需要继续修文档。

#### 文档修正要求

这份 review 文档应当立即回退为：

- 状态：`重新打开 / 进行中`
- 结论：`v3 解决了第一轮多数问题，但第二轮复审仍有 6 个未关闭 findings`

并把这 6 条新增 findings 作为”当前 gating”放在文档前部。

#### 修订说明

review 文档顶部状态已更新，6 个 finding 均已修复并回填。

## 第一轮 Findings（历史记录 / 大部分已修复）

### Finding 1 - 授权目录仍未进入真实 `send_message` 主链路

- 状态：`已修复`
- 严重级别：`P1`
- 文档定位：`/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:372`
- 真实代码定位：
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2596`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2722`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/session_runtime.rs:79`

#### 真实使用路径

用户发送消息：

`send_message -> chat_runtime_impl::agent_loop -> PluginContext -> ToolRegistry::execute`

#### 问题描述

文档仍把授权目录的关��注入点放在 `session_runtime.rs`，并假设后续工具执行会沿着 runtime context 往下消费；但当前真实生产聊天路径并不是由 `SessionRuntime` 驱动，而是在 `chat_runtime_impl.rs` 中直接构造 `PluginContext` 并执行工具。

也就是说，按文档实现后，`CapabilityContext.authorized_workspace` 很可能只存在于新 runtime 路径里，但真实生产聊天链路里拿到的仍是一个没有授权目录的 `PluginContext`。

#### 为什么这是 blocking issue

这会直接导致：

- 文档级 golden path 看起来成立
- 真实聊天里的 workspace 工具失效
- Python 授权目录上下文也可能只在理想路径生效，不在生产路径生效

#### 当前 TDD 风险

文档里的测试设计仍主要围绕 `session_runtime`、registry 和局部 dispatcher，没有强制证明真实 `chat_runtime_impl` 主链路已经拿到 `authorized_workspace`。

#### 文档修正要求

必须显式补上一条真实接线：

- `send_message` 当前版本如何把 session 级 `authorized_workspace` 注入到 `chat_runtime_impl.rs` 中构造的 `PluginContext`
- precompute 路径、普通工具路径、sub-agent 路径分别如何消费同一真相源

#### 修订说明

**修复方式：** 新增 §3.3「真实生产链路的授权目录注入」章节，明确：

- **注入点**：`chat_runtime_impl.rs:2596` 的 `PluginContext {}` 字面量构造处，从 `facade.authorized_workspace_store().get_current_for_session()` 查询并注入新字段 `authorized_workspace`
- **precompute 注入点**：`chat_runtime_impl.rs:1642` 的 sandbox 构建处，按有无授权目录选择 `for_workspace_with_authorized` 或 `for_workspace`；同时 `auto_load_ctx`（`:1651`）也补字段
- **schema 感知过滤**：`chat_runtime_impl.rs:1574` 附近，检查 `authorized_workspace_present` 后用 `ToolFilter::Exclude` 过滤或保留 workspace 工具
- 明确说明 `session_runtime.rs` / `CapabilityContext` 是未来架构目标，**本专项接线不走那里**
- 蓝图第 6 条（原 session_runtime.rs）改为「`chat_runtime_impl.rs`（真实注入点）」，详述三处代码修改

**新增测试：**
- `test_workspace_tools_dispatchable_via_registry` 证明 dispatcher 能找到工具（间接验证注入链路）
- checklist Phase W2 新增「chat_runtime_impl.rs 有 authorized_workspace_present 判断」项
- 文档定位：`/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:372`
- 真实代码定位：
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2596`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2722`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/session_runtime.rs:79`

#### 真实使用路径

用户发送消息：

`send_message -> chat_runtime_impl::agent_loop -> PluginContext -> ToolRegistry::execute`

#### 问题描述

文档仍把授权目录的关键注入点放在 `session_runtime.rs`，并假设后续工具执行会沿着 runtime context 往下消费；但当前真实生产聊天路径并不是由 `SessionRuntime` 驱动，而是在 `chat_runtime_impl.rs` 中直接构造 `PluginContext` 并执行工具。

也就是说，按文档实现后，`CapabilityContext.authorized_workspace` 很可能只存在于新 runtime 路径里，但真实生产聊天链路里拿到的仍是一个没有授权目录的 `PluginContext`。

#### 为什么这是 blocking issue

这会直接导致：

- 文档中的 golden path 成立
- 真实聊天里的 workspace 工具失效
- Python 授权目录上下文也可能只在理想路径生效，不在生产路径生效

#### 当前 TDD 风险

文档里的测试设计仍主要围绕 `session_runtime`、registry 和局部 dispatcher，没有强制证明真实 `chat_runtime_impl` 主链路已经拿到 `authorized_workspace`。

#### 文档修正要求

必须显式补上一条真实接线：

- `send_message` 当前版本如何把 session 级 `authorized_workspace` 注入到 `chat_runtime_impl.rs` 中构造的 `PluginContext`
- precompute 路径、普通工具路径、sub-agent 路径分别如何消费同一真相源

---

### Finding 2 - 未授权会话仍会暴露 workspace 工具

- 状态：`已修复`
- 严重级别：`P1`
- 文档定位：`/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:599`
- 真实代码定位：
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:1574`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_support.rs:443`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/plugin/registry.rs:112`

#### 真实使用路径

默认 daily / skill 聊天路径当前都是：

`ToolRegistry::get_schemas_filtered(ToolFilter::All)`

#### 问题描述

文档要求直接把 4 个 workspace 工具注册进 builtin registry，并接受 `ToolFilter::All` 下 schema 自动暴露给 LLM；但没有同时设计“只有存在 `authorized_workspace` 时才暴露 schema”的能力感知过滤机制。

结果会是：

- 所有普通未授权会话都能看到 `list_directory` / `read_workspace_file` / `search_files` / `get_file_info`
- 这些工具在大多数会话里只能返回 `No authorized workspace`
- 工具面被污染，模型会浪费轮次在不可用工具上

#### 为什么这是 blocking issue

这违背了 `workspace-first` 的目标。真正的 `workspace-first` 不是“把一堆目录工具总是摆给模型”，而是“当存在授权工作区时，模型看到并使用这组能力”。

#### 当前 TDD 风险

文档当前甚至把“`ToolFilter::All` 下多出 4 个 schema”写成完成标志之一，这是错误的验收方向。

#### 文档修正要求

必须在方案里明确：

- 无授权目录：workspace 工具 schema 不暴露
- 有授权目录：才暴露 schema
- 同时新增正反两组测试：
  - 有授权目录时 schema 出现
  - 无授权目录时 schema 不出现

#### 修订说明

**修复方式：** §3.3 明确了 schema 感知过滤规则：`chat_runtime_impl.rs` 在构造 `all_tool_defs` 前先查 `authorized_workspace_present`，无授权时使用 `ToolFilter::Exclude(WORKSPACE_TOOL_NAMES)` 排除 4 个工具，有授权时使用 `ToolFilter::All`。

**修正了原来的错误验收方向**：原文档把"`ToolFilter::All` 下多出 4 个 schema"写成完成标志，改为「有授权时 schema 列表包含，无授权时 schema 列表不包含」两组测试均通过。

**新增测试：**
- `test_workspace_schema_excluded_when_no_authorized_workspace`（无授权时 schema 被过滤）
- `test_workspace_tools_registered_in_tool_registry`（registry 中有工具，是前者过滤生效的前提）

---

### Finding 3 - W3 漏掉了 analysis precompute Python 路径

- 状态：`已修复`
- 严重级别：`P1`
- 文档定位：`/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:620`
- 真实代码定位：
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:1631`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:1642`

#### 真实使用路径

analysis / workflow 模式下存在一条前置 Python 主链路：

`step precompute -> SandboxConfig::for_workspace(&workspace_path) -> Python`

#### 问题描述

W3 只计划修改 `execute_python` handler，但真实分析流里的 precompute 仍直接调用 `SandboxConfig::for_workspace(&workspace_path)`。这意味着即使用户在手工 Python 工具里能访问授权目录，workflow / skill 的 deterministic precompute 仍然看不到授权目录。

#### 为什么这是 blocking issue

这会造成非常隐蔽的行为分裂：

- 日常聊天里某些 Python 代码能读授权目录
- 但 analysis 技能的 precompute 阶段不能读
- 用户看到的是“同一个会话，同一个目录，有时候能读，有时候不能读”

#### 当前 TDD 风险

文档里的 W3 测试只覆盖 `execute_python`，没有覆盖 analysis precompute 这条真实主链路。

#### 文档修正要求

W3 必须显式补上：

- precompute 也要走新的授权目录语义
- 补一条针对 analysis precompute 的集成测试或 golden trace

#### 修订说明

**修复方式：** W3 标题改为「Python Sandbox 扩展 + execute_python + analysis precompute 感知授权目录」，明确将 `chat_runtime_impl.rs:1642` 的 precompute sandbox 配置列入本期改造范围（§3.3 已给出具体代码）。

**新增测试：**
- `test_precompute_reads_authorized_directory` — 验证 precompute sandbox 的 `allowed_read_paths` 包含授权目录
- Phase W3 完成标志中增加「**precompute 集成测试**：`test_precompute_reads_authorized_directory` PASS」

---

### Finding 4 - 扩展 `allowed_paths` 会同时放开写授权目录

- 状态：`已修复`
- 严重级别：`P1`
- 文档定位：`/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:501`
- 真实代码定位：
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/python/sandbox.rs:168`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/python/sandbox.rs:226`

#### 真实使用路径

当前 Python 沙箱通过 `_ALLOWED_PATHS` 同时承担：

- 允许读取的根
- 允许写入的根

#### 问题描述

文档现在的设计是把授权目录直接 append 到 `allowed_paths`。但 `_safe_open` 的写权限检查是直接基于 `_ALLOWED_PATHS` 做 starts_with 判断的，所以一旦把授权目录放进去，Python 对该目录的写也一起放开了。

#### 为什么这是 blocking issue

这与文档自己在 `Not Doing` 里声明的“分析结果不直接写回授权目录”相冲突。也就是说，文档写的是“只读工作目录”，实际方案设计出来的是“读写工作目录”。

#### 当前 TDD 风险

文档目前只有“能读授权目录”的通过条件，没有“不能把结果写回授权目录”的负向测试。

#### 文档修正要求

必须在方案里二选一并写死：

1. 引入读写分离语义：`allowed_read_paths` 与 `allowed_write_paths`
2. 或显式声明本专项允许写授权目录，并同步修改问题定义 / Not Doing / UX 预期

按你们之前讨论的目标，更合理的是第 1 种：授权目录只读，Lotus workspace 继续承担输出目录。

#### 修订说明

**修复方式：** plan §3.6 和蓝图中的 sandbox 条目已改为读写分离：

- `SandboxConfig` 从单一 `allowed_paths` 拆为 `allowed_read_paths` / `allowed_write_paths`
- 授权目录只加入 `allowed_read_paths`
- `allowed_write_paths` 仅包含 Lotus workspace 的 7 个子目录
- `df.to_excel(_WORKSPACE_ROOT + '/output.xlsx')` 被明确写成负向示例，预期触发 `PermissionError`

**新增测试：**
- `test_for_workspace_with_authorized_read_path_includes_authorized`
- Phase W3 完成标志新增"写回授权目录触发 PermissionError"负向测试

---

### Finding 5 - Python `cwd` 语义与文档其它约束冲突

- 状态：`已修复`
- 严重级别：`P2`
- 文档定位：`/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:632`
- 相关文档定位：
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:505`
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:898`
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:988`

#### 问题描述

文档前面把 W3 设计成“注入 `_WORKSPACE_ROOT`，通过显式路径访问授权目录”，golden path 也是 `pd.read_csv(_WORKSPACE_ROOT + '/sales_2026.csv')`；但 W3 的完成标志又改成 `pd.read_csv('sales.csv')`，并声称 cwd 已切到授权目录。

#### 为什么这是问题

一旦 cwd 切到授权目录：

- 相对读路径默认指向授权目录
- 相对写路径也很容易落到授权目录
- 与“不直接写回授权目录”的约束进一步冲突

#### 当前 TDD 风险

文档没有固定一种 Python 运行语义，Claude 很容易做出：

- 一部分测试假设 `_WORKSPACE_ROOT`
- 一部分测试假设 cwd 已切换
- 最终实现与验收条件相互打架

#### 文档修正要求

建议直接定成：

- cwd 保持 Lotus workspace 不变
- 授权目录通过 `_WORKSPACE_ROOT` 显式访问
- 相对写路径继续落 Lotus workspace

并把文档、golden path、测试名统一成这一套语义。

#### 修订说明

**修复方式：** plan §3.6「Python 授权目录的读写语义」章节明确：

- cwd 固定为 Lotus workspace 根（`_ALLOWED_WRITE_PATHS[0]`），不改
- 授权目录通过 `_WORKSPACE_ROOT` 显式访问，不通过相对路径
- golden path 步骤 4 统一为 `pd.read_csv(_WORKSPACE_ROOT + '/sales_2026.csv')`（不用 `pd.read_csv('sales.csv')`）
- W3 完成标志中删去了「cwd 已切换到授权目录」的错误说法
- 全文 golden path、测试示例统一为 `_WORKSPACE_ROOT` 语义

---

### Finding 6 - store 没定义单 session 覆盖语义

- 状态：`已修复`
- 严重级别：`P1`
- 文档定位：`/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:321`
- 相关文档定位：`/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:986`

#### 真实使用路径

用户在同一个 session 中可能会重复选择目录：

- 第一次选 `/Users/alice/data-a`
- 第二次改选 `/Users/alice/data-b`

#### 问题描述

文档一边说“每个 session 只支持一个授权目录”，另一边 store contract 仍然是：

- `save(ws)`
- `get_by_session(session_id)`
- 生产实现按 `authorized_workspace:{uuid}` 存多条记录后扫 prefix 找第一条命中

这没有定义“同一个 session 第二次授权时如何替换旧值”的语义。

#### 为什么这是 blocking issue

如果没有 `replace_for_session` / `upsert_for_session` 语义：

- session 的当前目录真相源不稳定
- 查询结果依赖底层遍历顺序
- UI 看到的目录和 runtime 读到的目录都可能不一致

#### 当前 TDD 风险

文档里的测试只验证“能存、能按 session 查、session 之间隔离”，但没有验证“同一 session 重复授权后只保留一个当前目录”。

#### 文档修正要求

store contract 必须升级为显式单值语义，例如：

- `replace_for_session(session_id, workspace)`
- `get_current_for_session(session_id)`
- `clear_for_session(session_id)`

并补一条重复授权覆盖测试。

#### 修订说明

**修复方式：** store contract 升级为单值覆盖语义：

- 接口改为 `replace_for_session` / `get_current_for_session` / `clear_for_session`
- `FileAuthorizedWorkspaceStore` 以 `authorized_workspace:{session_id}` 为键（单条）直接覆盖
- `InMemoryAuthorizedWorkspaceStore` 用 `HashMap<session_id, AuthorizedWorkspace>` 自然实现单值
- §3.2 store contract 代码块、蓝图 §5 实现代码块、commands 调用处均同步更新
- W1 完成标志中 `test_save_and_get_by_session` 改名为 `test_replace_and_get_for_session`

**新增测试：**
- `test_replace_overwrites_previous`（同一 session 第二次授权后只保留新值）

---

### Finding 7 - 前端 `listAuthorizedDirectory` wrapper 没有后端合同

- 状态：`已修复`
- 严重级别：`P2`
- 文档定位：`/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:643`
- 真实代码定位：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/commands/workspace.rs:1`

#### 问题描述

W4 新增了 `src/lib/tauri.ts` 的 `listAuthorizedDirectory` wrapper，但前面的 command 设计只定义了：

- `authorize_local_directory`
- `get_authorized_workspace`
- `revoke_authorized_workspace`

没有任何对应的后端 IPC 命令或 adapter 设计来支撑 `listAuthorizedDirectory`。

#### 为什么这是问题

这会再次制造“前端 wrapper 已存在、后端命令不存在”的 UI/IPC 断层。

#### 当前 TDD 风险

文档只写了前端 wrapper，没有要求为它补 command contract test。

#### 文档修正要求

必须二选一：

1. 删除这个 wrapper，UI 目录浏览直接走已有 workspace 工具或已存在命令
2. 补完整后端合同：命令名、参数、返回结构、权限边界、测试

#### 修订说明

**修复方式：** 选择了方案 1——**删除这个 wrapper 设计**。

- W4 文件列表中不再包含 `src/lib/tauri.ts` 的 `listAuthorizedDirectory` wrapper
- 文档明确：UI 只通过 `get_authorized_workspace` 查询当前已授权目录的元信息
- 目录内容浏览不走额外 IPC，而是由 agent 使用 `list_directory` 工具完成
- 如未来确需纯 UI 文件树，再单开后端命令合同和测试，不在本专项范围内

---

### Finding 8 - 工具命名和上下文 contract 前后不一致

- 状态：`已修复`
- 严重级别：`P2`
- 文档定位：`/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:7`
- 相关文档定位：
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:478`
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md:495`

#### 问题描述

文档顶部 architecture 仍写：

- 工具名是 `read_file`
- 授权目录会注入 `ToolExecutionContext`

但后面的实际实现又变成：

- 工具名是 `read_workspace_file`
- 通过 legacy `ToolPlugin/PluginContext` 消费

#### 为什么这是问题

这不是纯措辞问题，而是 contract 问题：

- skill 的 allowed tool name 需要准确字符串
- registry / tests / prompt / 文档示例必须一致
- 后续专项 2 如果要从 legacy `ToolPlugin` 迁到 `RuntimeTool`，也需要一个统一的当前 contract

#### 当前 TDD 风险

如果不先统一命名和上下文 contract，Claude 很容易在：

- 测试文件
- registry 注册
- prompt 示例
- UI 文案

里分别落成两套名字。

#### 文档修正要求

必须统一成一套明确说法：

- 当前专项的正式工具名到底是什么
- 当前专项到底走 `PluginContext` 还是 `ToolExecutionContext`
- 如果是“短期 legacy + 后续专项 2 再迁移”，要在文档开头就写清楚，避免前后矛盾

#### 修订说明

**修复方式：** plan 顶部新增「工具名称 contract（本专项统一说法）」表，明确 4 个正式工具名分别是：

- `list_directory`
- `read_workspace_file`
- `search_files`
- `get_file_info`

同时在 Architecture 段落明确写清：**本专项统一走 `ToolPlugin` + `PluginContext`（legacy 生产路径），不走 `RuntimeTool` + `ToolExecutionContext`；后者留给专项 2 迁移。**

文档中所有示例、Phase W2、TDD 用例、Checklist 已统一为这一套名字和上下文 contract。

## 所有 P1/P2 已关闭，可进入代码实现

本轮 8 个 findings 均已修复（v3 文档）。建议执行顺序：

1. 按 Phase W1 → W2 → W3 → W4 顺序实现
2. 每个 Phase 完成后运行对应测试（见文档 §6 TDD 策略）
3. 实现完成后做一轮真正的 TDD / 行为回归 review

## 文档修订完成的最低通过条件（已全部满足）

- ✅ 真实 `send_message` 主链路如何拿到 `authorized_workspace` 被写清楚（§3.3，注入点在 `chat_runtime_impl.rs:2596`）
- ✅ workspace 工具的 schema 暴露具备”有授权才暴露”的规则与测试（§3.3，`test_workspace_schema_excluded...` 和 `test_workspace_tools_registered...`）
- ✅ analysis precompute 路径被纳入 W3（`chat_runtime_impl.rs:1642` 改造，`test_precompute_reads_authorized_directory`）
- ✅ Python 对授权目录的读写语义被明确拆分（§3.6，`allowed_read_paths` / `allowed_write_paths` 读写分离）
- ✅ store 具备单 session 单当前目录的覆盖语义（§3.2，`replace_for_session` / `get_current_for_session`，`test_replace_overwrites_previous`）
- ✅ 文档里的工具名、上下文 contract、IPC contract 统一成一套说法（顶部 contract 表，工具名 `list_directory` / `read_workspace_file` / `search_files` / `get_file_info`，统一走 `PluginContext`）
