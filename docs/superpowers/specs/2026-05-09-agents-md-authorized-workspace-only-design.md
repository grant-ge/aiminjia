# AGENTS.md 加载路径修复 — 仅读授权工作空间

**日期**:2026-05-09
**作者**:Claude Code(在 worktree `5c88/lotus-app` 内)
**前置工作**:`2026-05-08-prompt-architecture-fixes-design.md`(Wave 4 已完成 RenlijiaMd → AgentsMd 重命名,但保留旧加载逻辑未触)
**关联 plan**:本 spec 接受后单独写

---

## 1. 背景与问题

### 1.1 现状(bug)

GUI 验证 Wave 4 改造时发现:用户在已授权目录 `~/Documents/年度飞轮/` 下放了 `AGENTS.md`,**LLM 完全没有读到**(`66cfeb87-...` / `1a72d59a-...` 两个会话验证)。

根因:`AgentsMdLoader.load(workspace_path)` 收到的 `workspace_path = ~/.renlijia`(产物根目录,即 `AiJiaHome::from_home()`),而**不是**用户授权的项目目录(`authorized_workspace`,例 `~/Documents/年度飞轮`)。Loader 沿 `~/.renlijia` 父链找 AGENTS.md,自然查不到 `~/Documents/年度飞轮/AGENTS.md`。

### 1.2 旧实现的复杂度

`runtime/agents_md.rs` 当前查 4 类位置(去重后):

```
~/.renlijia/AGENTS.md                    (home 全局)
{workspace_path}/AGENTS.md
{workspace_path}/.aijia/AGENTS.md
{workspace_path}/AGENTS.local.md
+ workspace_path 的所有父目录上述三种
```

实际最深可能查 ~20 个位置(取决于路径深度)。

### 1.3 问题诊断

两层问题:

1. **路径源选错**:用 `workspace_path`(产物根)而非 `authorized_workspace`(项目根)
2. **加载策略过于复杂**:多层级 + 父链回溯 + `.aijia/` 子目录 + `AGENTS.local.md` 变体 + home 全局 — 用户难以预测"到底加载了哪个"
3. **多租户违反**:`~/.renlijia/AGENTS.md` 跨所有用户共享,违反 lotus 已建立的 `users/t_X__u_Y/` 分区

---

## 2. 决策

### 2.1 核心决策(用户确认)

**只读取 `{authorized_workspace}/AGENTS.md` 一个文件**。

- 没有授权目录(纯聊天) → 不加载任何 AGENTS.md
- 授权目录里没有 AGENTS.md → 不加载,不报错,不警告
- AGENTS.md 是空文件 → 加载但内容为空(等同没有)
- 仅查授权目录**正下方**这一层,不向上、不向下、不查 `.aijia/` 子目录、不查 `AGENTS.local.md` 变体

### 2.2 决策依据

| 维度 | 旧方案(多路径) | 新方案(单文件) |
|---|---|---|
| 用户心智模型 | 复杂,4 类位置 × 父链 | "项目目录里放就生效" |
| 多租户安全 | home 全局跨用户泄漏 | 天然按授权目录隔离 |
| 测试矩阵 | 路径组合爆炸 | 1 个文件,3 种状态(存在/不存在/空) |
| 代码复杂度 | ~70 行 + cache | ~15 行,无 cache 必要 |

### 2.3 显式废弃(不再支持的特性)

- ❌ `~/.renlijia/AGENTS.md`(home 全局)
- ❌ `{workspace}/.aijia/AGENTS.md`(子目录变体)
- ❌ `{workspace}/AGENTS.local.md`(local 变体)
- ❌ 父目录链上的任何 AGENTS.md
- ❌ 多文件叠加合并

迁移影响:**产品未发布,无历史用户**,无需兼容。

### 2.4 用户级 / 跨项目持久指令的替代方案

- **per-user 持久指令** → 走 lotus 已有的 `personas` 机制(per-user)或主对话 `system_prompt_extra`
- **跨项目共享** → 用户自行在每个项目放一份 AGENTS.md(显式 > 隐式)

---

## 3. 加载契约

### 3.1 输入

调用方:`RuntimeChatTurnDriver` / `WorkerRuntime`(子代理) / 测试 executor。
输入:授权目录引用(`Option<AuthorizedWorkspaceRef>`)。

### 3.2 输出

```rust
Vec<AgentsMdFile>  // 长度 ∈ {0, 1}
```

- `Vec::new()`:无授权目录 / 文件不存在 / 文件读取失败(失败时打 warn)
- 长度为 1:授权目录正下方有 `AGENTS.md`,内容已读出(可能为空字符串)

### 3.3 缓存语义

按 `(canonical_path, mtime)` 缓存内容,避免每轮 turn 重读磁盘。
mtime 变化 → 失效重读。
文件被删除 → 调用方收到 `Vec::new()`,缓存条目随之失效。

### 3.4 大小限制

**新增**:AGENTS.md 文件大小 > 64 KiB 时,**截断**到 64 KiB 并打 warn(`agents_md_truncated`)。
理由:旧实现没有这个保护,用户传一个 10MB 的 AGENTS.md 会污染整个上下文窗口。

---

## 4. 改动清单

### 4.1 `runtime/agents_md.rs`(简化)

**删除**:
- `home_dir()` 函数
- `~/.renlijia/AGENTS.md` 加载分支
- 父目录链遍历代码
- `.aijia/AGENTS.md` 变体
- `AGENTS.local.md` 变体
- `seen: HashSet<PathBuf>` 去重逻辑(单文件不需要)

**保留**:
- `AgentsMdFile` 结构
- `AgentsMdLoader` 结构(仅留 mtime 缓存)
- `read_with_cache()` 私有方法

**改签名**:
```rust
// 旧
pub async fn load(&mut self, workspace_path: &Path) -> Vec<AgentsMdFile>

// 新
pub async fn load(
    &mut self,
    authorized_workspace: Option<&AuthorizedWorkspaceRef>,
) -> Vec<AgentsMdFile>
```

**新增**:文件大小截断检查。

**模块文档头**(`//!`)同步重写,反映"只读授权目录正下方一个文件"的语义。

### 4.2 `runtime/chat/chat_turn_driver.rs`

**trait 默认实现**(line ~298)签名同步改为接收 `Option<&AuthorizedWorkspaceRef>`,**不再**接收 `_workspace_path: &Path`。
默认实现仍返回 `Ok(vec![])`(测试 executor 无需感知 authorized_workspace)。

**调用点**(line ~1144-1155):
```rust
// 旧
let agents_md_files = executor
    .load_agents_md(&config.workspace_path)
    .await
    .unwrap_or_else(|e| { ... });

// 新
let agents_md_files = executor
    .load_agents_md(config.authorized_workspace.as_ref())
    .await
    .unwrap_or_else(|e| { ... });
```

依赖 `ChatTurnConfig` 已有 `authorized_workspace: Option<AuthorizedWorkspaceRef>` 字段;若没有,补一个(见 4.3)。

### 4.3 `ChatTurnConfig`(`runtime/chat/...` 内)

**确认 / 补充字段**:
```rust
pub authorized_workspace: Option<AuthorizedWorkspaceRef>,
```

`build_driver_for_turn` 在构造 config 时,从 `chat_runtime_impl::load_authorized_workspace(app, conversation_id)` 取值塞进去。

(已知 `worker_runtime.rs` / `tool_executor` 已经传 `authorized_workspace`,这里只是把它再多挂到 `ChatTurnConfig`。)

### 4.4 `transport/tauri_commands/chat.rs`

**`TauriLegacyTurnExecutor::load_agents_md`**(line 1463-1469):
```rust
async fn load_agents_md(
    &self,
    authorized_workspace: Option<&AuthorizedWorkspaceRef>,
) -> Result<Vec<crate::runtime::agents_md::AgentsMdFile>, TurnError> {
    let mut loader = self.agents_md_loader.lock().await;
    Ok(loader.load(authorized_workspace).await)
}
```

字段 `agents_md_loader` 保留(loader 内部状态变了,接口变了)。

### 4.5 删除测试 / 改写测试

**删除**(逻辑已不存在):
- 测父链遍历的测试
- 测 home 全局的测试
- 测 `.aijia/` / `AGENTS.local.md` 的测试
- 测多文件去重的测试

**新增 / 改写**:
- `loads_when_authorized_workspace_has_agents_md`
- `returns_empty_when_authorized_workspace_is_none`
- `returns_empty_when_file_not_present`
- `returns_empty_when_file_is_empty_string`(空文件,长度 1 的 Vec,内容空)
- `truncates_when_file_exceeds_64kib`
- `mtime_cache_hits_on_unchanged_file`
- `mtime_cache_invalidates_on_modification`

---

## 5. 验收清单

### 5.1 自动化

- `cargo test --test agents_md_test`(或对应集成测试)全绿
- `cargo test base_md_includes_agents_md_rule`(Wave 4 测试不破)
- `cargo test --test subagent_persona_test`(Wave 4 测试不破)
- `cargo check` 0 error,新增 warn ≤ 现状

### 5.2 GUI 手测(用户操作)

| # | 场景 | 期望 |
|---|---|---|
| 1 | 授权 `~/Documents/年度飞轮`,该目录有 AGENTS.md(写"用户名 pzcaaaa"),开新对话问"我叫什么" | AI 回答"pzcaaaa" |
| 2 | 同上但删除该 AGENTS.md,开新对话问"我叫什么" | AI 不知道 |
| 3 | 授权目录改为另一个没有 AGENTS.md 的目录,问"我叫什么" | AI 不知道(确认不再读父链 / home) |
| 4 | 不授权任何目录(纯聊天),问"我叫什么" | AI 不知道(确认 None workspace 不挂) |
| 5 | 授权目录里放一个 100KB 的 AGENTS.md,问任意问题 | 后台日志出现 `agents_md_truncated`,功能不报错 |

### 5.3 后端日志校验

每轮 turn 应出现以下日志之一:
- `agents_md loaded path=... bytes=N`(成功)
- `agents_md absent path=...`(文件不存在)
- `agents_md skipped reason=no-authorized-workspace`(无授权目录)

---

## 6. 不在本次范围

| 项 | 处理 |
|---|---|
| per-user 持久指令的 UI(类似 `~/.renlijia/users/.../AGENTS.md` 编辑器) | 不做,后续 feature 可基于 personas 实现 |
| AGENTS.md 多文件叠加 | 不做(显式简单) |
| AGENTS.md 中的 `@import` / `source` 跨文件引用 | 不做 |
| 子代理(SubAgent)的 AGENTS.md 加载 | 子代理沿用主对话已加载的 AGENTS.md 内容(由 `chat_turn_driver` 注入到 user-context message),子代理路径不重新加载磁盘 |
| 前端 UI 提示用户"AGENTS.md 已加载" | 不做(本次修后可见性靠后端日志) |

---

## 7. 风险与回滚

### 7.1 风险

- **风险 A**:`ChatTurnConfig` 没有 `authorized_workspace` 字段,需要新增 — 触动 turn 配置 struct,影响范围中等。
  - **缓解**:`worker_runtime.rs` 已有先例,跟着模式抄。
- **风险 B**:测试数据放在临时目录(`tempfile`),需要构造 `AuthorizedWorkspaceRef` mock。
  - **缓解**:`AuthorizedWorkspaceRef` 是简单 struct,直接构造即可。
- **风险 C**:用户依赖 home 全局 AGENTS.md 的工作流被破坏。
  - **缓解**:产品未发布,无历史用户,无需兼容。

### 7.2 回滚

git revert 单个 plan commit 即可恢复旧逻辑(本 spec 改动相对独立,不与 Wave 1-4 耦合)。

---

## 8. 待用户确认

- [x] 只读 `{authorized_workspace}/AGENTS.md`(单文件,不向上)
- [x] 完全废弃 home 全局 / `.aijia/` 子目录 / `AGENTS.local.md` 变体
- [ ] 64 KiB 文件大小上限是否合适(默认建议值,可调)
- [ ] 是否需要 `agents_md absent` / `loaded` 这类标准化日志(默认推荐)

如有调整告诉我。否则我直接进 plan。
