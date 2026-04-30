# Skill 系统完整修订计划（待执行）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在不破坏 Plan-U 主线收口成果的前提下，补齐 skill 在 runtime 中的“可感知、可约束、可迁移”能力，并逐步与 claude-code-best 的 SKILL.md 生态对齐。

**Architecture:** 分三期推进：期一只打通 runtime 链路（skill prompt + 工具约束实际生效）；期二把激活从硬编码关键词推进到 runtime-tool 驱动并支持中途切换；期三再处理格式与存储统一（SKILL.md + skills 目录），避免与 storage unification/skill 管理改造互相打架。

**Tech Stack:** Rust / Tauri 2.x / tokio / serde_json / toml（期三引入 markdown+frontmatter 解析）

---

## 执行位置 / 依赖关系 / 非目标 / 先决条件

### 执行位置（主线定位）

- 本计划**不是**插队到 Plan-U 之前的新主线。
- 本计划定位为：`Plan-U 主线（U→V→AA→AB→AC→AD→AE→W→X→Y→Z→AF）收口后的后续专项`。
- 若主线程需要提前做期一中的局部链路打通（例如仅做观测/埋点），也必须以“不回退 U 系列边界”为硬约束。

### 依赖关系（最低依赖）

- 至少依赖以下能力已落地并稳定：
  - `Plan-U3`：上下文预处理管道（避免 skill 注入破坏统一 preprocess 顺序）；
  - `Plan-U6`：PluginContext 热路径退出（避免 skill 链路重新绑回 legacy bridge）；
  - `Plan-U5`：worker runtime 收口（保证主线程/worker 的 skill 行为语义一致）。
- 与 `2026-04-20-plan-storage-unification.md`（`~/.renlijia/skills`）以及后续 skill 管理目录统一计划强耦合：期三必须复用该目录策略，不得另起一套路径规范。

### 非目标（本计划不做）

- 不在本计划内重建 legacy `ToolPlugin + PluginContext` 执行主链。
- 不恢复或新增“双轨 prompt pipeline”（daily/analysis 双 loop）。
- 不在期一/期二引入跨端同步、远程 skill marketplace、云端 skill 托管。

### 先决条件与分期约束

- 期一可以先做 runtime 链路接通，但**必须**满足：
  - 不把 `PluginContext` 拉回 runtime 热路径；
  - 不把 legacy tool bridge 重新变成默认执行入口；
  - 不在 `run_chat_turn_s4` 外再平行复制一套 prompt/tool 过滤管道。
- 期二/期三衔接原则：
  - 期二必须对齐当前 runtime tool 现状（`src-tauri/src/runtime/tools/*` + `src-tauri/src/plugin/registry.rs`），不引用不存在的 registry 结构；
  - 期二完成后直接进入期三执行，不再回头展开其他计划，也不在两期之间额外停下来征求确认；
  - 只有在 skill 计划本身被现实实现阻塞时，才允许先修计划文本再继续实现；
  - 期三必须对齐当前 storage 现状（`AiJiaHome.skills_dir()`、`scan_external_plugins`、`skill_management` 仍基于 plugin.toml 的迁移窗口），采用“迁移窗口 + 明确切换点”，不能硬切导致主线失稳。

---

## 背景：当前问题（按主线兼容性重排）

| 编号 | 问题 | 影响 |
|---|---|---|
| B1 | skill 激活检测未进入 turn 主循环 | skill 不会真实影响当轮行为 |
| B2 | system prompt 未携带 skill 上下文 | 模型对当前 skill 无感知 |
| B3 | `TurnConfig.allowed_tools` 仍未驱动 `ToolRoundDriver` 过滤 | 工具白名单语义未闭环 |
| B4 | skill 会话态缺少稳定持久化/恢复策略 | 重连或跨轮行为不一致 |
| B5 | `DeclarativeSkill::should_activate` 限于 `daily-assistant` | 仅能单向切入，无法多 skill 循环 |
| B6 | 激活机制以关键词硬匹配为主 | 误触发/漏触发，且与 runtime-tool 化方向冲突 |
| B7 | 缺少 mid-conversation 切换通道 | 用户需求变化时切换成本高 |
| B8 | 缺少路径/上下文条件激活建模（与 preprocess/workspace 信息联动） | 无法按上下文自动切换 |
| B9 | skill 文件格式与存储目录仍是旧形态（plugin.toml + prompts） | 与 claude-code-best / storage-unification 方向冲突 |
| B10 | 内置 skill prompt 更新路径重编译成本高 | 迭代慢、难热更新 |

---

## 代码落点总览（按当前仓库真实路径）

### 期一：runtime 链路接通（B1-B3 + B4 会话内一致性）

- `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- `src-tauri/src/runtime/chat/turn_config.rs`
- `src-tauri/src/runtime/chat/tool_round_driver.rs`
- `src-tauri/src/runtime/session_runtime.rs`
- `src-tauri/src/runtime/chat/mod.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/transport/tauri_commands/chat.rs`
- `src-tauri/tests/*skill*`（新增/改造测试）

### 期二：激活机制升级（B5-B8）

- `src-tauri/src/runtime/tools/`（新增 skill 切换 runtime tool）
- `src-tauri/src/runtime/tools/definition.rs`
- `src-tauri/src/runtime/tools/catalog.rs`（如需 schema 暴露）
- `src-tauri/src/plugin/builtin/tools/mod.rs`
- `src-tauri/src/plugin/registry.rs`（当前真实 ToolRegistry 所在）
- `src-tauri/src/plugin/declarative_skill.rs`
- `src-tauri/src/transport/tauri_commands/chat.rs`
- `src-tauri/src/llm/prompts.rs`

### 期三：格式与目录统一（B9-B10）

- `src-tauri/src/runtime/store/session_store.rs`
- `src-tauri/src/storage/file_store/mod.rs`
- `src-tauri/src/plugin/declarative_skill.rs`
- `src-tauri/src/plugin/manifest.rs`
- `src-tauri/src/plugin/mod.rs`
- `src-tauri/src/commands/skill_management.rs`
- `src-tauri/src/lib.rs`（`scan_external_plugins`）
- `scripts/`（迁移脚本）
- `src-tauri/plugins/*`（迁移窗口内逐步转化）

---

# 期一：执行链路打通（最小闭环）

**目标：** 让 skill 对当轮 LLM 行为和工具执行产生可验证影响；只做 runtime 闭环，不引入新桥接。

> **B4 边界修正：** 本期只解决“当前进程 / 当前会话内”的 skill 状态一致性；不把 `SessionRecord` 字段扩展误当成跨重连或跨重启恢复闭环。真正的持久化恢复放到期三单列任务处理。

## 任务 P1-1：会话态与 driver 入参对齐

- [ ] 以 `skill_session.rs`（或同等职责模块）作为当前进程内的主状态源，管理会话内 skill id / step / runtime state。
- [ ] 在 `src-tauri/src/runtime/session_runtime.rs` 增加 skill 相关依赖注入（仅 runtime-first 结构），不引入 `PluginContext`。
- [ ] 在 `src-tauri/src/runtime/chat/mod.rs` 导出新增 skill 会话管理模块（如创建 `skill_session.rs`）。
- [ ] 若需要在 `SessionRecord` 增加 skill 相关字段，只能作为元数据占位或调试辅助；不得把它视为跨重启恢复闭环。
- [ ] 把“跨重连 / 跨重启 skill 恢复”明确延期到期三的持久化任务，并提前写清真实存储落点必须落在 conversation metadata 或专用持久化结构，而不是只改内存 store。

## 任务 P1-2：turn 内 skill 注入与工具约束闭环

- [ ] 在 `src-tauri/src/runtime/chat/chat_turn_driver.rs` 的 `run_chat_turn_s4` 中接入 skill 激活/读取逻辑。
- [ ] 在 `TurnConfig` 构建阶段注入 skill prompt 后缀。
- [ ] 在真实生产 wiring 路径 `src-tauri/src/transport/tauri_commands/chat.rs` 中同步接通 skill 依赖，不能只停留在 `session_runtime.rs` / `lib.rs` 的局部注入。
- [ ] **关键修正：** `TurnConfig.allowed_tools` 当前为 `Option<HashSet<String>>`，而 `ToolRoundDriver` 当前消费 `Option<Vec<String>>`；本期要统一二者边界（二选一：driver 接受 `HashSet` 或 config 在进入 round 前做稳定转换）。
- [ ] 把 `allowed_tools` 真正传入 `ToolRoundDriver::with_allowed_tools_opt(...)`，避免“配置有值但执行层忽略”。
- [ ] **补齐模型可见工具面：** 不只闭环执行时 allowlist，还要同步改造 `get_tool_defs()` / tool schema 暴露路径，让模型可见工具集与当前 skill 的执行工具集一致。
- [ ] 若期一暂不引入 `switch_skill`，也必须先保证“skill 激活后，模型可见工具面”和“执行时允许工具面”使用同一份 skill 约束来源。

## 任务 P1-3：期一验证

- [ ] 新增/改造测试覆盖：
  - 激活后 system prompt 包含 skill 信息；
  - 激活后模型可见工具定义与 skill 允许工具集一致；
  - 激活后工具白名单对 blocked/permitted 分支生效；
  - 无匹配时保持 default skill，不污染现有 daily 行为。
- [ ] 至少包含一个回归测试，验证未启用 skill 时行为与当前主线一致。

## 期一验收

- [ ] skill prompt 注入在主循环生效；
- [ ] 模型可见工具面与执行时工具面保持一致；
- [ ] tool allowlist 从 TurnConfig 到 ToolRoundDriver 全链路生效；
- [ ] 无 `PluginContext`/legacy bridge 热路径回流。

---

# 期二：激活机制升级（runtime-tool 化）

**目标：** 从“关键词驱动切换”升级为“runtime tool + 模型决策 + 显式状态变更”，并支持 mid-conversation 切换。

## 任务 P2-1：Skill 切换工具 runtime 化

- [ ] 在 `src-tauri/src/runtime/tools/` 新增 skill 切换工具（建议 `skill_switch.rs`，文件名以实际实现为准）。
- [ ] 在 `src-tauri/src/runtime/tools/definition.rs` 使用现有 `ToolKind` 体系（当前为 Primitive/Power/Composite/Support），不新增不存在的 `Meta` 枚举值。
- [ ] 在 `src-tauri/src/plugin/builtin/tools/mod.rs` 完成注册。

## 任务 P2-2：注册面与 schema 面对齐当前架构

- [ ] **关键修正：** ToolRegistry 当前位于 `src-tauri/src/plugin/registry.rs`，不得引用不存在的 `src-tauri/src/runtime/tools/registry.rs`。
- [ ] 若 skill tool 依赖 `SkillRegistry`，采用显式参数注入：在 builtin runtime tool 注册函数或 setup wiring 中直接传入，不得给 `ToolRegistry` 新增全局 `skill_registry` 字段。
- [ ] 在本计划中明确禁止把 `ToolRegistry` 再扩张为 service locator；该约束与 `Plan-U6` 保持一致。
- [ ] 如需让 LLM看见新工具，更新 `src-tauri/src/runtime/tools/catalog.rs` 与 `get_tool_defs` 产物的一致性校验。

## 任务 P2-3：激活策略从硬编码降级为兜底

- [ ] 调整 `src-tauri/src/plugin/declarative_skill.rs`：去除 `current_skill == "daily-assistant"` 的硬门槛，仅保留防重复激活与兜底规则。
- [ ] 在 `src-tauri/src/transport/tauri_commands/chat.rs` / `src-tauri/src/llm/prompts.rs` 注入“可用 skill + 切换工具”说明，保持与现有 system prompt 组装入口一致。
- [ ] 明确：关键词触发只作 fallback，主路径为 runtime tool 显式切换。

## 任务 P2-4：mid-conversation 切换与状态传播

- [ ] 先定义“tool -> driver”的显式返回协议，再做切换实现；**不要**只把待切换信号塞进 `ToolExecutionContext`。
- [ ] 推荐方案：扩展 `ToolDispatchOutcome::Completed`（或等价 driver 可见结果结构），附带 `next_skill_id` / `runtime_state_patch` 一类字段，再由 turn driver 在下一轮消费。
- [ ] 若最终不扩展 `ToolDispatchOutcome`，也必须在计划中明确替代协议（例如 `ToolResult` 结构化 metadata），避免实现时临时拍脑袋。
- [ ] 覆盖主线程 + worker 场景，确保与 U5 的 worker runtime 语义一致。

## 期二验收

- [ ] 模型可通过 runtime tool 触发 skill 切换；
- [ ] 同一会话可多次切换，不依赖回到 daily 才能切换；
- [ ] 不新增 legacy prompt pipeline 或 `PluginContext` 依赖。

---

# 期三：格式与存储统一（SKILL.md 迁移窗口）

**目标：** 与 storage unification 和当前 skill 管理现状对齐，完成“可回滚、可观测”的格式迁移，而非一次性硬切。

## 任务 P3-0：补齐跨重连 / 跨重启恢复落点

- [ ] 明确 B4 的真实持久化落点：conversation metadata 或专用 skill session persistence；不得把 `SessionRecord` 扩展本身当成恢复闭环。
- [ ] 当前最小落点定为专用 skill session persistence：先把 skill state 序列化到 conversation-scoped memory key（建议键形如 `note:{conversation_id}:active_skill_state`），由生产 `SkillSessionStore` 在新建 runtime 后优先恢复；后续若迁到 conversation metadata，也必须保证读写窗口兼容。
- [ ] 打通 load / save / reconnect 路径，让 skill 状态恢复能被真实生产链路消费，而不是只停留在 store 结构变化。
- [ ] 为“当前会话内一致性”和“跨重启恢复”分别写回归测试，防止两者继续混为一谈。

## 任务 P3-1：定义双读窗口与切换点

- [ ] 在 `src-tauri/src/plugin/declarative_skill.rs` 定义迁移窗口策略：
  - 当前窗口第一阶段先保持 `plugin.toml` 为主、`SKILL.md` 为兜底，确保现有 skill 生态不被硬切打断；
  - 对历史 `plugin.toml + workflow.toml + prompts` 保持临时兼容读取，并允许仅含 `SKILL.md` 的目录先进入扫描/管理链路；
  - 在后续里程碑版本再显式切到 `SKILL.md` 优先，并最终移除旧格式读取。
- [ ] 在计划文档中明确“移除旧格式”的具体里程碑，不在本次修订内直接硬切。

## 任务 P3-2：目录统一与扫描入口对齐

- [ ] `scan_external_plugins`（`src-tauri/src/lib.rs`）扫描策略与 `AiJiaHome.skills_dir()` 保持一致。
- [ ] 迁移后目录不得与 `2026-04-20-plan-storage-unification.md` 冲突（统一落在 `~/.renlijia/skills`）。

## 任务 P3-3：模板与管理命令迁移

- [ ] 更新 `src-tauri/src/commands/skill_management.rs` 的脚手架输出，支持生成 SKILL.md（并在迁移窗口内可选输出旧结构）。
- [ ] `skill_management`、`skill_smith`、扫描入口、热重载入口必须在迁移窗口内提供可回归的双读 / 双写策略，不能只做“评估兼容点”。
- [ ] 在移除旧格式前，至少保证以下链路均已通过回归：
  - 扫描可加载旧格式与新格式；
  - 管理命令可生成旧格式与新格式；
  - 热重载可处理旧格式与新格式；
  - 模板输出与加载器识别规则一致。

## 任务 P3-4：批量迁移工具与验证（后置收口项）

- [ ] 先以前三项把双读 / 双写窗口跑通；**迁移脚本不是当前阶段 blocker**，不得先于持久化恢复和 `skill_smith` / 管理链路闭环抢占主线。
- [ ] 只有当 `src-tauri/plugins/*` 或外部 skill 目录进入批量切换阶段、手工双写成本已不可接受时，才在 `scripts/` 下补充迁移脚本（文件名可用 `migrate_skill_to_md.py`），并要求支持 dry-run。
- [ ] 若补充迁移脚本，先做样本迁移再做全量迁移，并记录失败清单；同时增加加载回归测试，确保新旧格式在窗口内均可加载，并可按开关切换。

## 期三验收

- [ ] 跨重连 / 跨重启的 skill 恢复有真实持久化落点，而不是仅靠 `SessionRecord` 字段扩展；
- [ ] 目录、扫描、模板、加载器四者一致；
- [ ] 与 storage unification 路径规范无冲突；
- [ ] 旧格式退出前，双读 / 双写窗口已通过回归验证；
- [ ] 可通过开关/版本策略安全退出旧格式。

---

## 与主线计划的衔接说明（避免打架）

- 本计划在执行排期上服从 `Plan-U -> Plan-V -> Plan-AA -> Plan-AB -> Plan-AC -> Plan-AD -> Plan-AE -> Plan-W -> Plan-X -> Plan-Y -> Plan-Z -> Plan-AF`。
- 若主线程判断需要提前“预研/铺路”，仅允许期一中不改变热路径边界的准备项（测试、接口预留、注释约束）。
- 任何使 `PluginContext`、legacy tool bridge、双轨 prompt pipeline 回流热路径的改动，一律视为违背本计划。

---

## 风险与决策点（提交主线程裁决）

1. **`allowed_tools` 类型错位风险**：`TurnConfig(HashSet)` 与 `ToolRoundDriver(Vec)` 当前接口不一致，若不先统一，期一会产生“看似接通但实际无效”的假完成。
2. **格式迁移窗口风险**：`skill_management`/`skill_smith` 仍深度依赖 `plugin.toml`；若期三直接 hard cut 到 SKILL.md，会造成命令链断裂。
3. **注册层归属风险**：计划历史文本把 ToolRegistry 误写到 `runtime/tools/registry.rs`，但真实在 `plugin/registry.rs`；若继续按旧路径推进会直接偏航。
4. **Skill / Agent 双重裁决风险**：若后续 `AgentDefinition` 与 skill 都能控制工具面，必须先确定最终裁决权，否则容易形成双重过滤或互相覆盖。

---

## 完成定义（DoD）

- [ ] 计划文本全部使用当前真实路径（`src-tauri/src/...`、`scripts/...`）。
- [ ] 不再引用不存在文件或旧架构前提。
- [ ] 明确前置依赖、非目标、迁移窗口和验收边界。
- [ ] 文档保持“待执行”状态，不宣称任何任务已落地。
