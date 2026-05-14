# Team 协作两处持久化 bug 修复方案

> 日期：2026-05-14
> 状态：待执行
> 作者：lotus-app team
> 对照参考：`/Users/a20250311/github/claude-code-best`

## 背景

近期重构了 `team-chat.jsonl`（teammate 群聊持久化）+ Path A wake（Lead 续接 turn）+ teammate addendum prompt，主对话视图已经能完整看到团队过程。但实测发现 teammate 启动后 transcript 里反复出现两类工具错误：

```
tool execution failed: Not a file: /Users/.../users/t_28__u_54/conversations/<id>/team.json
TaskList → No tasks found（即使 Lead 已 TaskCreate）
```

派 3 个并行 subagent 调研后确认，**team / tasks 两个共享数据通道都有持久化 bug**，导致 teammate 看不到 Lead 创建的 task、也读不到团队成员名册。

## 两个 bug 总览

| # | Bug | 现象 | 严重度 |
|---|---|---|---|
| 1 | `team.json` 永远不写盘 | `TeamRegistry::persist()` 函数已实现但**全仓库 0 调用点**，teammate prompt 给的 `团队配置: <conv_dir>/team.json` 路径永远是空文件 | 中 |
| 2 | task 路径分裂 | task 工具实际写到 `~/.renlijia/conversations/<conv>/tasks/`（非 user-scoped），但 prompt 告诉 teammate 去 `~/.renlijia/users/t_X/conversations/<conv>/tasks/`（user-scoped）。**两条路径不交叉**，导致 Lead 和 teammate 实际操作的是不同目录 | 高 |

Bug #2 比 Bug #1 严重——task 列表是 Lead/teammate 协作的核心数据通道，路径分裂导致 task 完全失效。

## claude-code-best 对照

| 维度 | claude-code-best | lotus-app 当前 |
|---|---|---|
| team config 文件 | `~/.claude/teams/{teamName}/config.json` ✅ 真写 | `<conv_dir>/team.json` ❌ persist 函数定义了但 0 调用 |
| tasks 存储 | `~/.claude/tasks/{taskListId}/{taskId}.json` ✅ Lead/teammate 共享 | `<conv_dir>/tasks/{taskId}.json` ✓ 设计共享但**实际写到错误的根目录** |
| `taskListId` 取值逻辑 | `setLeaderTeamName(teamName)` + `getTeammateContext().teamName`，Lead/teammate 解析到同一个 | 用 `conv_id` 做隐式 listId，Lead/teammate 同一会话本来就应该共享 |
| teammate prompt 怎么找 team config | 给路径 + 让 LLM `Read` 文件 | 同样给路径 + 让 LLM `Read`（**prompt 设计正确，照搬 claude-code-best**） |
| teammate prompt 怎么找 tasks | 给路径 + 让 LLM `Read` 目录 / 调 `TaskList` 工具 | 同上（**prompt 设计正确**） |

**结论**：lotus 的 prompt 跟 claude-code-best 一模一样（中文翻译版），**设计没问题**，问题在持久化端没把数据写到 prompt 承诺的路径。

## Bug 1：team.json 永远不写盘

### 根因

`src-tauri/src/runtime/agent/team.rs:141` 的 `TeamRegistry::persist(session, conv_dir)` 是完整可用的，签名：

```rust
pub async fn persist(
    &self,
    session_id: &SessionId,
    conv_dir: &Path,
) -> Result<(), TeamPersistError>
```

但全仓库 `grep "team_registry.*\.persist|reg\.persist"` 0 个调用点。`TeamRegistry::delete_persisted(conv_dir)` 同样未被调用。

teammate boot prompt（`src-tauri/src/runtime/agent/team_context.rs:23`）渲染：
```
- 团队配置: {team_json_path}    ← 真实展开成 <conv_dir>/team.json
读取团队配置文件了解队友名单。
```

teammate LLM 第一轮直接调 `Read({path: team.json})` → 文件不存在 → 工具报错 → teammate 在自己 transcript 里抱怨"未找到 team.json"。

### 修复

在团队生命周期 4 个事件点调 `persist` / `delete_persisted`。改动量 ~40 行，零接口变更。

| # | 文件 | 位置 | 改动 | 行数 |
|---|---|---|---|---|
| 1.1 | `src-tauri/src/runtime/tools/builtin/team_tools.rs` | `TeamCreateRuntimeTool::execute` 内 `register_lead_inbox` 之后 | `ctx.team_registry().persist(&session, conv_dir).await` | +6 |
| 1.2 | `src-tauri/src/runtime/tools/builtin/spawn_subagent.rs` | `team_guard.add_teammate(member)` 成功之后（释放 lock 之后） | 同上，`tokio::spawn` fire-and-forget 避免阻塞 tool 返回 | +10 |
| 1.3 | `src-tauri/src/runtime/agent/worker_runtime.rs` | `cleanup_teammate()` 末尾 `team.remove_teammate(name)` 之后 | 同上，需从 `TeammateLlmEngine.team_registry` 取 registry 引用 | +8 |
| 1.4 | `src-tauri/src/runtime/tools/builtin/team_tools.rs` | `TeamDeleteRuntimeTool::execute` 内 `registry.delete(&session).await` 之后 | `TeamRegistry::delete_persisted(conv_dir)`（同步函数，不需 await） | +5 |

每处都用 `if let Some(conv_dir) = ctx.conv_dir.as_ref()` 保护，conv_dir 为 None 时静默跳过（生产路径下已经在 #SessionRuntime 注入，None 主要是单测场景）。

失败处理：所有 persist 失败只 `log::warn!`，不让 tool 返回错误——这是 best-effort 镜像，内存 registry 是 source-of-truth。

### 风险

- spawn_subagent 用 `tokio::spawn` 异步 persist：如果 teammate 还没等到 persist 完成就 boot 起来，第一次 Read team.json 仍会 NotFound。需要看 spawn 实际时序：先 `add_teammate` → 写 meta.json → start IdleLoop → teammate 第一轮 Read。中间有上百毫秒，async persist 大概率赶在 Read 之前完成；如果实测仍有竞态，改成同步 await 即可。

## Bug 2：task 路径分裂

### 根因

`src-tauri/src/runtime/tools/builtin/task_tools.rs:92-108` 的 `store_for`：

```rust
fn store_for(ctx: &ToolExecutionContext) -> Result<FileTaskV2Store, ToolError> {
    let home = ctx
        .task_store_root
        .clone()
        .or_else(|| ctx.capability.as_ref().and_then(|c| c.storage.as_ref()).map(|s| s.workspace_path.clone()))
        .or_else(default_aijia_home)
        .ok_or_else(...)?;
    let conv_id = ctx.session_id.as_str();
    let tasks_root = home.join("conversations").join(conv_id).join("tasks");
    Ok(FileTaskV2Store::new(tasks_root))
}

fn default_aijia_home() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".renlijia"))
}
```

三个 home 候选都**不包含 `users/{scope}/` 前缀**：
- `task_store_root` —— 全代码库 0 处注入，永远 None
- `capability.storage.workspace_path` —— 是工作区目录，跟会话存储无关
- `default_aijia_home` —— `~/.renlijia/`，直接挂根

结果 tasks 写到 `~/.renlijia/conversations/<conv>/tasks/`（实测确认 `8b229f22` 会话目录就是这条），但：
- 会话主目录是 `~/.renlijia/users/t_28__u_54/conversations/<conv>/`（user-scoped）
- teammate boot prompt 给的路径是 `~/.renlijia/users/t_28__u_54/conversations/<conv>/tasks/`（user-scoped）
- team_view.rs / team_chat.jsonl / messages.jsonl 全部用 user-scoped

**task 工具是仓库里唯一一个走非 user-scoped 路径的模块**。

### 修复

`ctx.conv_dir` 在生产路径下已经是完整的 user-scoped 会话目录（由 `SessionRuntime` 通过 `QueryEngine::with_conv_dir` 注入）。直接用它，不再走 home 拼接。

| # | 文件 | 位置 | 改动 |
|---|---|---|---|
| 2.1 | `src-tauri/src/runtime/tools/builtin/task_tools.rs:92-108` | `store_for` 函数 | 优先用 `ctx.conv_dir.as_ref()`，fallback 才走原 home 拼接（兼容单测） |

伪代码：

```rust
fn store_for(ctx: &ToolExecutionContext) -> Result<FileTaskV2Store, ToolError> {
    // 生产路径：直接用注入的 conv_dir（已是 user-scoped）。
    if let Some(conv_dir) = ctx.conv_dir.as_ref() {
        return Ok(FileTaskV2Store::new(conv_dir.join("tasks")));
    }
    // Fallback：单测或老代码路径。
    let home = /* 原 task_store_root / capability / default_aijia_home 链 */;
    let conv_id = ctx.session_id.as_str();
    Ok(FileTaskV2Store::new(home.join("conversations").join(conv_id).join("tasks")))
}
```

### 历史数据迁移

修复后，**已存在的 task 仍在错位置** `~/.renlijia/conversations/<conv>/tasks/`，teammate 看不到。两个处理思路：

a) **不迁移**：修复后新会话立即生效。旧会话的旧 task 留在错位置不管（反正这些会话大概率已经过期）。**推荐**。

b) **写迁移脚本**：扫旧路径 → 拷贝/移动到 user-scoped 路径。复杂度高，收益低。

## 修复执行顺序

1. **先修 Bug 2（task 路径）**——影响协作核心数据通道，~10 行单点改动，风险低
2. **再修 Bug 1（team.json persist）**——4 处独立 patch，可以分两次提交
3. 编译 + 启动 dev + 跑辩论赛会话验证：
   - 会话目录下应同时存在 `team.json` + `tasks/` 两个 user-scoped 子项
   - teammate transcript 里不应再出现 `Not a file: .../team.json`
   - teammate 调 `TaskList()` 应该能看到 Lead 创建的 task

## 验证脚本

```bash
CONV=~/.renlijia/users/t_28__u_54/conversations/<新会话>
# 必须都存在
test -f $CONV/team.json && echo "✓ team.json"
test -d $CONV/tasks && echo "✓ tasks/"
# 不应该出现错位置
test ! -d ~/.renlijia/conversations/<新会话>/tasks && echo "✓ no leak to non-user-scoped"
# teammate transcript 不应再有 NotFound
grep -L "Not a file.*team.json" $CONV/teammates/*.jsonl && echo "✓ no team.json NotFound"
```

## 与之前修复的关系

本文档涉及的两个 bug **跟之前 `team-chat.jsonl` / Path A wake / teammate addendum** 三处修复**正交**，那三处针对的是"消息通信链路"，本文档针对的是"配置/任务持久化"。

提交建议拆分：
- `fix(team): persist team.json on TeamCreate / spawn / cleanup / TeamDelete`
- `fix(task): use ctx.conv_dir for user-scoped tasks/ path`
