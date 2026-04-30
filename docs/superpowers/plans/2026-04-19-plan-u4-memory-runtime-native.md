# 本地记忆 Runtime-Native 化（Plan-U4）

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:test-driven-development` — 先锁记忆格式、加载顺序、注入位置，再动实现。 REQUIRED SUB-SKILL: `superpowers:verification-before-completion` — 必须验证加载、召回、写回和 legacy 退场四条链路。

**Goal:** 把 lotus 当前 `core_memory + legacy memory tools` 的混合态，升级为本地文件式、runtime-native 的记忆主线；记忆的加载、召回、写回都不再依赖 legacy `ToolPlugin`。

**Architecture:** 对标 `claude-code-best/docs/context/project-memory.mdx` 的本地文件记忆思路，但只做 lotus 本地桌面需要的最小闭环：以 canonical workspace 为 key 的本地 memory 目录、入口索引、运行时召回、显式写回服务；不做团队记忆、远端同步、向量库。

**Tech Stack:** Rust, Markdown, local file storage

**Worktree branch:** pzc

---

## 背景与现状

| 文件 | 现状 |
|---|---|
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | 当前只会加载一个 `core_memory` 字符串，注入方式也还停留在早期简化形态 |
| `src-tauri/src/plugin/builtin/tools/mod.rs` | `memory_save / memory_search / memory_core / memory_distill` 全部仍标注为 legacy、且没有 RuntimeTool 替代 |
| `src-tauri/src/llm/tool_executor/memory.rs` | 记忆读写仍依赖 `PluginContext`，属于 legacy tool-executor 链 |
| `src-tauri/src/runtime/claude_md.rs` | 已有项目级 `CLAUDE.md` 加载，但它不是项目记忆系统本身 |

### 为什么这条链路还没闭环

- `core_memory` 只是一个大字符串，不是可组织、可召回、可验证的记忆结构。
- 记忆能力仍绑定在 legacy tool 家族上，运行时主链路无法稳定依赖它。
- `CLAUDE.md`、`core_memory`、legacy memory tool 现在是三套概念，没有统一入口。

## 范围

- 纳入：
  - 本地文件式 memory 目录与入口索引
  - runtime loader / recall / write-back 服务
  - 记忆注入位置、召回规则、legacy memory tool 退场
- 不纳入：
  - 团队记忆、共享仓库记忆
  - 向量检索、远程同步、云端记忆
  - 与代码库无关的通用知识库产品面

## 任务拆分

### U4-1：定义本地 memory 存储模型

- [ ] 新建 `ProjectMemoryService` 与对应的文件布局；默认按 canonical workspace 映射到应用数据目录中的 memory bucket。
- [ ] 入口文件采用 `MEMORY.md` 或等价索引，正文记忆拆成独立条目文件，不再把所有内容揉成一个 `core_memory` 字符串。
- [ ] 约束 memory 类型与 frontmatter，至少区分用户偏好、项目约束、参考信息三类。

### U4-2：实现 runtime 读取与相关性召回

- [ ] 在 turn 启动前，通过 `ProjectMemoryService` 加载入口索引与相关 memory 条目。
- [ ] 记忆作为独立 runtime context 注入，不与 `CLAUDE.md`、日期提醒、普通 user message 混写。
- [ ] 若 `Plan-U3` 已落地，记忆注入顺序应挂到新的 pre-processing / context assembly 主线中。

### U4-3：实现写回与蒸馏入口

- [ ] 把保存记忆、更新索引、蒸馏记忆做成 runtime-native service / command，而不是 legacy `memory_*` ToolPlugin。
- [ ] 明确哪些内容允许写进 memory，避免把可从代码实时推导的信息再存一遍。
- [ ] 写回必须带结构化 metadata，方便未来 review 与清理。

### U4-4：让 legacy memory tools 退场

- [ ] 停止在 prompt / tool surface 中暴露 legacy `memory_*` 工具名。
- [ ] 保留兼容迁移窗口，但生产主路径不再依赖 `llm/tool_executor/memory.rs`。
- [ ] 对已存在的 `core_memory` 内容提供一次性迁移脚本或 lazy migration 方案。

### U4-5：回归测试

- [ ] 覆盖 memory index 读取、相关条目召回、注入顺序、写回更新、迁移兼容。
- [ ] 增加 review test，防止后续又把 runtime memory 回退成一块不可审计的大字符串。
- [ ] 验证没有 legacy memory tool 的情况下，主对话仍能读取与使用本地记忆。

## 验收标准

- 记忆有本地文件式结构，而不是单一 `core_memory` blob。
- 记忆加载、召回、写回都走 runtime-native 主线。
- legacy `memory_*` 工具退出生产主路径。
- 整条记忆链路保持本地-only，不依赖远程服务。
