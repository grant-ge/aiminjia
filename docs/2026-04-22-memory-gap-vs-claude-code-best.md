# Memory 架构 Gap 分析：对标 claude-code-best

> **调研方法**：4 个并行 agent 分别探索后端 Rust 实现、前端 React 实现、docs 参考文档、事件/hook 链路，综合汇总。
> **对标基准**：`claude-code-best/docs/context/project-memory.mdx`（源码路径 `src/memdir/`）
> **调研日期**：2026-04-22

---

## 零、背景与目标

### 为什么做这件事

AIjia（AI小家）的核心差异化是"懂你的 AI 工作台"——AI 需要跨对话记住用户偏好、项目约束、历史纠正，每次对话都能更好地配合用户工作，而不是每次重头来过。

当前 memory 系统（Plan-U4 阶段落地的 `ProjectMemoryService`）已建立了文件式存储结构，但主链路存在关键断裂：**LLM 无法主动写记忆，召回质量低，前端对记忆系统零感知**。这使得"懂你"的体验无法真正建立起来。

本文档的目的是：以 claude-code-best 的 `src/memdir/` 实现为对标基准，**精确定位当前实现与对标之间的差距**，为后续修复提供依据。

### 希望达成的结果

修复完成后，memory 系统应满足：

1. **写回闭环**：LLM 在对话中能主动保存、更新、蒸馏记忆，无需用户手动干预
2. **语义召回**：每次 turn 开始时，根据当前对话内容语义召回最相关的记忆条目（而非字面匹配）
3. **完整分类**：支持四类型分类（user / feedback / project / reference），尤其是 feedback 类型用于持久化 AI 行为矫正信号
4. **Session 级状态**：跨 turn 共享同一个 memory 上下文，避免重复注入同一条记忆
5. **前端可见**：用户能看到记忆被保存/召回，能手动管理记忆列表
6. **注入规范化**：记忆内容和记忆指导分两条通道注入，支持 Prompt Cache 优化

### 与 Plan-U4 的关系

Plan-U4（`docs/superpowers/plans/2026-04-19-plan-u4-memory-runtime-native.md`）已识别了核心问题并制定了任务拆分，但调研发现存在 5 个 Plan-U4 未覆盖的盲区（G3/G7/G8/G9/G10，详见第三节）。本文档作为 Plan-U4 的补充诊断，后续应据此更新 Plan-U4 或新建 Plan-U4-v2。

---

## 一、当前架构现状

### 1.1 三条独立 Memory 通路

lotus-app 存在三个相互独立、定位不同的 memory 子系统：

| 通路 | 存储路径 | 用途 | 注入 Prompt |
|------|---------|------|------------|
| **Cognitive Memory** | `shared/cognitive/mem.md` + `daily/*.jsonl` | 跨对话用户知识库，LLM 主动写入 | ✅ `[核心记忆]` 块 |
| **Project Memory** | `project_memories/{hash}/entries/*.md` | workspace 级项目约束/偏好，前端写入 | ✅ `[项目记忆]` 块（与 Cognitive 互斥） |
| **Enterprise Memory** | `shared/memory/memory.jsonl` | KV 存储（PII 映射、Skill 状态、已加载文件） | ❌ 从不注入 |

**关键互斥逻辑**（`chat_turn_driver.rs` 第 755-765 行）：

```rust
let core_memory_str = if project_memory_ctx.is_empty() {
    executor.load_core_memory(...).await
} else {
    String::new()  // 有 project_memory 时 cognitive memory 被强制跳过
};
```

Cognitive Memory 和 Project Memory **不能同时注入**。

### 1.2 Memory 注入链路（当前 S4 路径）

```
run_chat_turn_s4()
  ├── load_project_memory(workspace_path, user_query)
  │     └── ProjectMemoryService::load_context(query)
  │           ├── ensure_legacy_migrated()      # 懒迁移旧 mem.md
  │           ├── load_entries()                # 读 entries/*.md
  │           └── select_relevant_entries()     # 词元匹配，top-5
  │
  ├── [if project_memory is empty]
  │   load_core_memory()
  │     └── 读取 shared/cognitive/mem.md 全文
  │
  └── build_iteration_context(core_memory, project_memory_ctx, env_info, ...)
        └── dynamic_context 字符串 → 每次 iteration 重复注入 LLM
```

### 1.3 Memory 写入触发方式

| 写入路径 | 触发者 | 状态 |
|---------|-------|------|
| `save_memory` 工具 | LLM 主动调用 | ❌ **已注释禁用** |
| `distill_memories` 工具 | LLM 主动调用 | ❌ **已注释禁用** |
| `save_project_memory` Tauri command | 前端 UI | ✅ 注册但**无 TS 封装** |
| `distill_project_memory` Tauri command | 前端 UI | ✅ 注册但**无 TS 封装** |

**核心问题**：LLM 目前无法主动写入任何 memory。

---

## 二、对标 claude-code-best：逐项比对

### 2.1 存储结构对比

| 维度 | claude-code-best | lotus-app | 状态 |
|------|-----------------|----------|------|
| 存储根路径 | `~/.claude/projects/<sanitized-git-root>/memory/` | `{app_data}/project_memories/{workspace-slug-hash}/` | ✅ 思路一致 |
| 入口索引 | `MEMORY.md`（200行/25KB 双重上限） | `MEMORY.md`（无上限保护） | ⚠️ 缺大小上限 |
| 条目格式 | 独立 `.md` 文件 + YAML frontmatter | 独立 `.md` 文件 + YAML frontmatter | ✅ 一致 |
| 多 worktree 共享 | 通过 `findCanonicalGitRoot()` 共享 | 按 workspace 路径 hash，不同 worktree 视为不同 bucket | ⚠️ worktree 场景下可能分裂 |
| 路径安全保护 | 故意排除 `projectSettings` 覆盖，防止恶意仓库指向 `~/.ssh` | 无此保护 | ⚠️ 潜在安全点 |

### 2.2 记忆分类系统对比

| 类型 | claude-code-best | lotus-app |
|------|-----------------|----------|
| `user` | ✅ 用户角色、偏好、技术背景 | `user_preference`（近似） |
| `feedback` | ✅ AI 行为纠正和确认（双通道） | ❌ **缺失** |
| `project` | ✅ 非代码可推导的项目上下文 | `project_constraint`（近似） |
| `reference` | ✅ 外部系统指针 | `reference_info`（近似） |

**feedback 类型缺失的影响**：无法持久化用户对 AI 行为的纠正（"别 mock 数据库"）和确认（"对，就是这样"），AI 行为将随时间漂移。

### 2.3 召回机制对比

| 维度 | claude-code-best | lotus-app | 状态 |
|------|-----------------|----------|------|
| 召回方式 | Sonnet 侧查询（`sideQuery()`，独立 API 调用） | 词元匹配（字符串包含，按词频打分） | ❌ 无语义 |
| 语义理解 | ✅ 理解意图 | ❌ 只匹配字面词 | ❌ |
| 中文支持 | ✅ Sonnet 原生理解中文 | ❌ 单字（<2字节）被过滤，中文分词缺失 | ❌ |
| 最大召回数 | 5 条 | 5 条（`MAX_RECALLED_ENTRIES = 5`） | ✅ |
| 去噪 | `recentTools` 参数：跳过当前正在使用的工具相关记忆 | ❌ 无 | ❌ |
| 去重 | `alreadySurfaced` 参数：跳过本轮已展示的记忆 | ❌ 无 | ❌ |
| 工具陷阱优先 | ✅ 即使跳过工具文档，工具的警告/陷阱仍保留 | ❌ 无此区分 | ❌ |

### 2.4 注入链路对比

| 维度 | claude-code-best | lotus-app |
|------|-----------------|----------|
| 注入时机 | Session 启动时一次（`getSystemContext()` 缓存） | Turn 启动时，`build_iteration_context()` 每 iteration 重建 |
| 注入位置 | `MEMORY.md` 内容 → user context message（利用 Prompt Cache prefix 共享） | `dynamic_context` 字符串，混入 env_info / precompute 等 |
| 记忆内容和指导分离 | ✅ `memory-mechanics` 指导在 system section，记忆内容在 user message | ❌ 混写，无分离 |
| 跨 turn 去重 | `QueryEngine` 持有 `loadedNestedMemoryPaths: Set<string>`，同 session 内同一文件不重复加载 | ❌ 每次 turn 新建 `ProjectMemoryService` 实例，无跨 turn 状态 |
| Cognitive + Project 兼容 | 不适用（单一体系） | ❌ 两者互斥，不能同时注入 |

### 2.5 写回机制对比

| 维度 | claude-code-best | lotus-app |
|------|-----------------|----------|
| 模型写回方式 | FileWrite/FileEdit 工具直接操作记忆文件 | ❌ 四个 memory 工具全部注释禁用 |
| 写回指导 | `memory-mechanics` prompt 告知文件名、写法语义 | ❌ 无对应 prompt section |
| 前端触发 | 不需要 | `save_project_memory` 命令已注册但无 TS 封装 |
| 自动蒸馏 | `/dream` 技能（KAIROS 模式）+ 手动 `/compact` | `needs_cognitive_distill()` 已实现但**无任何调用点** |

### 2.6 记忆漂移防御对比

| 维度 | claude-code-best | lotus-app |
|------|-----------------|----------|
| 漂移防御 prompt | ✅ "Before recommending from memory" 专用 section | ❌ 无 |
| 忽略记忆的严格语义 | ✅ 明确区分"忽略"vs"承认后覆盖" | ❌ 无 |
| 文件/函数验证要求 | ✅ 引用文件路径前 check 是否存在，函数名 grep 确认 | ❌ 无 |

### 2.7 Session Memory 与压缩联动对比

| 维度 | claude-code-best | lotus-app |
|------|-----------------|----------|
| 压缩联动 | `sessionMemoryCompact`：直接用记忆文件作压缩摘要，无额外 API 调用 | ❌ 压缩走独立 API，记忆系统与压缩完全解耦 |

---

## 三、Gap 清单

### P1 — 主流程断裂（直接影响功能完整性）

**G1：模型无法主动写回记忆**
- 现状：`save_memory`、`search_memory`、`load_core_memory`、`distill_memories` 四个工具全部注释禁用（`plugin/builtin/tools/mod.rs` 第 93-96 行）。`DAILY_ALLOWED_TOOLS` 不含任何 memory 工具。
- 补充：`AppStorage::needs_cognitive_distill()` 和 `cognitive::needs_auto_distill()` 已完整实现 24 小时自动蒸馏判断逻辑，但**全代码库无任何调用点**。蒸馏只能靠 LLM 工具调用（已禁用）或前端命令手动触发，形成双重断路。
- 影响：记忆无法在对话中自动积累，已有记忆也不会被自动整理蒸馏。
- 对标：模型通过 FileWrite/FileEdit 工具直接操作记忆文件，`memory-mechanics` prompt 指导写法；`/dream` 技能或 `/compact` 触发蒸馏。

**G2：召回是字符串匹配，无语义理解**
- 现状：`select_relevant_entries()` 做词元计数（`query_tokens` 过滤 <2 字符的词），中文单字被跳过，中文分词缺失。查询"分析销售数据"无法召回 `description: "用户偏好直接使用 Python 分析"` 的条目。
- 影响：记忆召回有效率低，尤其是中文语境下。
- 对标：Sonnet `sideQuery()` 做语义召回，理解意图而非字面匹配。

**G3：缺少 `feedback` 类型分类**
- 现状：三类（`user_preference / project_constraint / reference_info`），无 `feedback` 类型。
- 影响：用户的纠正指令（"别 mock 数据库"）和确认信号（"对，就是这样"）无法持久化，AI 行为随时间漂移。
- 对标：`feedback` 类型专门捕获双通道信号（失败纠正 + 成功确认），是防漂移的核心机制。

**G4：无 Session 级 memory owner，跨 turn 状态不复用**
- 现状：`load_project_memory` 每次 turn 新建 `ProjectMemoryService` 实例（`chat.rs`），无状态复用。
- 影响：无法实现跨 turn 的 `alreadySurfaced` 去重，每 turn 都可能重复注入同一记忆。
- 对标：`QueryEngine` 跨 turn 持有 `loadedNestedMemoryPaths: Set<string>`，同 session 内不重复加载。

**G5：legacy `tool_executor/memory.rs` 未退场（Plan-U4 验收标准未达）**
- 现状：`handle_save_memory` / `handle_search_memory` / `handle_load_core_memory` 仍依赖 `PluginContext`，被 `tool_executor/mod.rs` re-export，属于 legacy 链。
- 影响：双轨并存，Plan-U4 要求 "legacy memory tools 退出生产主路径" 尚未完成。
- 对标：Plan-U4 U4-4 任务项。

### P2 — 质量与成本（影响效率和用户体验）

**G6：记忆注入位置不当，无 Prompt Cache 优化**
- 现状：记忆内容混入 `dynamic_context` 字符串（与 env_info、precompute_result 等动态内容拼接），每 iteration 重建。
- 影响：每次 iteration 都重注入记忆内容，token 浪费；无法利用 Prompt Cache 的 prefix 共享。
- 对标：`MEMORY.md` 内容作为独立 user context message 注入；记忆指导 vs 记忆内容分两条通道。

**G7：无记忆漂移防御 prompt section**
- 现状：召回的记忆直接拼接进 prompt，无任何"记忆内容可能过期"的提示。
- 影响：模型会基于过期的文件路径/函数名给出错误建议。
- 对标：专用 system prompt section "Before recommending from memory"，要求验证文件/函数是否仍存在。

**G8：MEMORY.md 无大小上限保护**
- 现状：`rebuild_index()` 直接拼接所有条目的 `render_index_line()`，无行数或字节上限。
- 影响：长期使用后 MEMORY.md 可能撑爆 context window。
- 对标：`MAX_ENTRYPOINT_LINES = 200`，`MAX_ENTRYPOINT_BYTES = 25_000`，双重上限，超出自动截断并追加警告。

**G9：无 recentTools 去噪 / alreadySurfaced 去重**
- 现状：`load_context(query)` 每次全量召回，无"本轮已展示过的记忆跳过"逻辑，无"当前正在使用某工具时跳过该工具文档"逻辑。
- 影响：记忆槽浪费，重复注入同一内容。
- 对标：`findRelevantMemories(query, alreadySurfaced, recentTools)` 三参数协同。

**G10：无 Session Memory 与上下文压缩联动**
- 现状：`compact_summary` 走独立 API 调用，记忆系统与压缩完全解耦。
- 影响：长对话压缩慢、成本高；已有记忆未被复用。
- 对标：`sessionMemoryCompact` 直接用记忆文件作压缩摘要，无额外 API 调用。

---

## 四、前端完全失明

前端对 memory 系统几乎没有感知：

| 缺失项 | 现状 | 影响 |
|--------|------|------|
| Memory 管理 UI | 无（SettingsModal 无 Memory Tab） | 用户无法查看/删除/触发 distill |
| `save_project_memory` TS 封装 | 命令已注册，`tauri.ts` 无封装函数 | 前端无法触发 project memory 保存 |
| `Persona.memoryHints` 编辑控件 | `PersonaTab.tsx` 只暴露 4 个字段，无 memoryHints 编辑器 | 用户无法配置 persona 记忆提示 |
| Post-turn memory 触发 | `useStreaming.ts` 的 `onTurnCompleted` 只清理 UI 状态 | 无自动后处理 |
| 记忆召回可见性 | 无任何 RuntimeEvent 通知，用户不知道记忆系统在工作 | 系统透明度为零 |
| 记忆写入可见性 | 无 `MemorySaved` 事件 | 同上 |

---

## 五、事件层缺口

`RuntimeEventKind`（`events.rs`）**完全没有** memory 相关变体：

- 缺 `MemoryLoaded` / `MemoryInjected`（turn 开始加载了 memory，前端无感知）
- 缺 `MemorySaved`（LLM 写入 memory，前端无感知）
- 缺 `MemoryRecalled`（召回了哪些条目，无可观测性）

另：`RunStarted`、`RunCompleted`、`RunCancelled`、`OrphanedPermissionDetected` 被 `tauri_event_adapter.rs` 的 `_ => None` 静默丢弃，前端完全不可见。

---

## 六、规划盲区（Plan-U4 未覆盖的 Gap）

Plan-U4（`docs/superpowers/plans/2026-04-19-plan-u4-memory-runtime-native.md`）已识别 G1（写回）、G4（Session owner）、G5（legacy 退场），但以下 5 项**在 Plan-U4 文档中完全未被提及**：

| Gap | 描述 |
|-----|------|
| G2 | Sonnet 语义召回（Plan-U4 只说"相关性召回"，未明确 sideQuery） |
| G3 | feedback 类型分类（Plan-U4 定义三类型，漏掉 feedback） |
| G7 | 记忆漂移防御 prompt section |
| G8 | MEMORY.md 大小上限 |
| G9 | recentTools 去噪 / alreadySurfaced 去重 |

---

## 七、推荐修复优先级（快速索引）

> 详细任务拆分见第九节。此处仅给出阶段划分和关键路径。

```
阶段一（写回闭环）  → T1 T2 T3 T4  — 不完成则记忆永远无法自动积累
阶段二（召回质量）  → T5 T6 T7 T8  — feedback 类型和 Session owner 是前置
阶段三（注入规范化）→ T9 T10 T11 T12
阶段四（前端可见）  → T13 T14 T15  — 依赖 T1/T13
阶段五（长对话优化）→ T16          — 依赖 Plan-U3 + Plan-K
```

---

## 八、关键结论

> lotus-app 的 memory 系统处于**"结构已定，主线断裂"**的状态：
> - 存储层（`ProjectMemoryService` + 文件布局）方向正确，与 claude-code-best 对标一致
> - 但**写回闭环（G1）被人工切断**：四个工具全部禁用，LLM 无法主动写记忆
> - **召回质量（G2）差距最大**：字面匹配 vs Sonnet 语义召回，中文场景下尤其严重
> - **Session 层（G4）缺失**：无跨 turn 状态，去重去噪全部失效
> - **前端完全失明**：无事件、无 UI、无 TS 封装，用户对记忆系统零感知

核心修复顺序：**G1（写回）→ G5（legacy 退场）→ G3（feedback 类型）→ G4（Session owner）→ G2（语义召回）**

---

## 九、可执行任务拆分（Plan-U4 补全版）

> 本节补全 Plan-U4 遗漏的 5 个盲区，并与原 Plan-U4 任务合并为统一执行列表。
> 标注 `[U4原有]` 的任务来自 Plan-U4，`[新增]` 的任务是本次调研新识别的。

### 阶段一：写回闭环（最高优先级，不完成则记忆无法自动积累）

- [ ] **T1** `[U4原有 U4-3]` 实现 `WriteMemoryTool` 作为 RuntimeTool，替代 legacy `memory_save` ToolPlugin
  - **为什么不用 FileWrite/FileEdit 直接写记忆目录**：claude-code-best 能这样做是因为记忆目录在 `~/.claude/projects/` 下，是沙箱外的路径；lotus-app 的 `project_memories/` 在 `AppData` 下，现有沙箱允许写，但路径格式（hash bucket）和 frontmatter 格式需要封装层保证，直接 FileWrite 容易写出格式错误的条目。中期可评估放开后统一到 FileWrite。
  - 实现 `RuntimeTool` trait，输入参数：`name / type / description / content`，调用 `ProjectMemoryService::save_memory()`
  - 注册到 `ToolRegistry`，加入 `DAILY_ALLOWED_TOOLS`
  - 新增 `memory-mechanics` system prompt section：告知模型何时保存、保存什么类型、frontmatter 写法示例

- [ ] **T2** `[U4原有 U4-3]` 实现 `SearchMemoryTool` 作为 RuntimeTool，替代 legacy `memory_search` ToolPlugin
  - 暂用现有词元匹配召回，G2（Sonnet sideQuery）在阶段二替换

- [ ] **T3** `[U4原有 U4-4]` 让 legacy memory 代码正式退场
  - `plugin/builtin/tools/memory_save/search/core/distill.rs` → 删除（已注释，彻底移除）
  - `llm/tool_executor/memory.rs` → 停止 re-export，移除 `PluginContext` 依赖
  - 保留 `ensure_legacy_migrated()` 数据迁移逻辑（仅迁移，不调用旧工具）

- [ ] **T4** `[新增]` 为 `save_project_memory` / `distill_project_memory` Tauri command 补充 TypeScript 封装
  - 在 `src/lib/tauri.ts` 中添加对应 invoke 函数
  - 前端应急路径：用户可通过 UI 手动触发写回

### 阶段二：分类与召回质量

- [ ] **T5** `[新增]` 添加 `feedback` 类型
  - 扩展 `ProjectMemoryType` 枚举，添加 `Feedback` 变体
  - 更新 frontmatter 文档，补充 `when_to_save`（纠正 + 确认双通道）、`body_structure`（rule + Why + How to apply）
  - 更新模型侧 `memory-mechanics` prompt，明确 feedback 写法示例

- [ ] **T6** `[U4原有 U4-2]` 将 `ProjectMemoryService` 实例提升至 Session 级 owner
  - `QueryEngine` 或 `SessionRuntime` 持有单一 `ProjectMemoryService` 实例，跨 turn 复用
  - 持有 `already_surfaced: HashSet<String>`（已展示条目 path），传入 `load_context()` 过滤

- [ ] **T7** `[新增]` 在 `load_context()` 中引入 `recent_tools` 去噪参数
  - 函数签名：`load_context(query, already_surfaced, recent_tools) -> ProjectMemoryContext`
  - 当 `recent_tools` 包含某工具时，跳过该工具的使用文档类条目（但保留其警告/陷阱类条目）

- [ ] **T8** `[新增]` 引入 Sonnet sideQuery 作为召回后端（可选，优先级低于 T5-T7）
  - 替换 `select_relevant_entries()` 中的词元匹配
  - 依赖 `LlmGateway`，需通过 `CapabilityContext` 或独立注入
  - 前置条件：T6 完成（Session 级 owner 才能缓存 sideQuery 开销）

### 阶段三：注入规范化

- [ ] **T9** `[新增]` 在 system prompt 中添加 "Before recommending from memory" section
  - 参考 `claude-code-best/src/memdir/memoryTypes.ts` 的 `TRUSTING_RECALL_SECTION`
  - 内容：引用文件路径前检查是否存在，引用函数名前 grep 确认；"忽略记忆"的严格语义定义

- [ ] **T10** `[新增]` 为 `MEMORY.md` 的 `rebuild_index()` 添加双重上限
  - `MAX_INDEX_LINES = 200`，`MAX_INDEX_BYTES = 25_000`
  - 超出时截断并追加警告行

- [ ] **T11** `[新增]` 将记忆内容从 `dynamic_context` 字符串中提取为独立注入通道
  - 记忆内容（`[项目记忆]` 块）不再与 env_info / precompute 等动态内容混拼
  - 作为独立 user context message 注入，与 system prompt 中的记忆指导分离
  - 前置条件：Plan-U3 上下文主线落地（context assembly 主线）

- [ ] **T12** `[新增]` 修复 Cognitive Memory 与 Project Memory 互斥逻辑
  - 移除 `if project_memory_ctx.is_empty()` 的互斥条件
  - 两者按优先级叠加注入（Project Memory 在前，Cognitive Memory 在后）
  - 需评估 token 预算影响

### 阶段四：可观测性与前端体验

- [ ] **T13** `[新增]` 在 `RuntimeEventKind` 中添加 memory 相关事件
  - `MemoryLoaded { count: usize }`（turn 开始时加载了 N 条记忆）
  - `MemorySaved { name: String, memory_type: String }`（LLM 写入了一条记忆）
  - 在 `tauri_event_adapter.rs` 中添加对应的 Tauri event 映射

- [ ] **T14** `[新增]` 前端 Memory 管理 UI（SettingsModal 新增 Memory Tab）
  - 展示当前 workspace 下所有 project memory 条目
  - 支持查看条目详情、手动删除、手动触发 distill
  - 展示 Cognitive Memory（`mem.md`）内容预览

- [ ] **T15** `[新增]` `Persona.memoryHints` 编辑控件
  - `PersonaTab.tsx` 添加 memoryHints tag-list 编辑器

### 阶段五：长对话优化（低优先级，依赖外部条件）

- [ ] **T16** `[新增]` 研究 Session Memory 与 compact 联动的可行性
  - 前置条件：Plan-U3 上下文主线、autocompact（Plan-K）落地
  - 目标：压缩时直接复用已有记忆文件，减少摘要 API 调用

---

### 任务依赖关系

```
T1 (WriteMemoryTool)
T2 (SearchMemoryTool)
T3 (legacy 退场)    ─── 都可并行启动
T4 (TS 封装)
     │
     ▼
T5 (feedback 类型)
T6 (Session owner)  ─── T1-T3 完成后启动
T7 (recent_tools 去噪)
     │
     ▼
T8 (Sonnet sideQuery) ← 依赖 T6
T9 (漂移防御 prompt)
T10 (MEMORY.md 上限)  ─── T5-T7 完成后启动
T11 (独立注入通道)    ← 依赖 Plan-U3
T12 (互斥逻辑修复)
     │
     ▼
T13 (RuntimeEvent)
T14 (Memory 管理 UI) ← 依赖 T13
T15 (memoryHints UI)
     │
     ▼
T16 (compact 联动)   ← 依赖 Plan-U3 + Plan-K
```

### 验收标准（整体）

1. LLM 在对话中调用 `write_memory` 工具后，条目出现在 `project_memories/{hash}/entries/` 下
2. Turn 开始时，`MemoryLoaded` 事件通知前端召回了哪些条目
3. `feedback` 类型条目能被正确保存和召回
4. 同一 session 内同一条目不重复注入（`already_surfaced` 生效）
5. legacy `tool_executor/memory.rs` 不再在生产路径中被调用（review test 覆盖）
6. `MEMORY.md` 超过 200 行时自动截断并追加警告
