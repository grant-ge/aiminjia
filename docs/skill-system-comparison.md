# Skill 系统深度对比：lotus-app vs claude-code-best

> 调查日期：2026-04-21 | 分支：pzc

---

## 一、Skill 格式

### claude-code-best

单文件 `SKILL.md`，frontmatter 是配置，body 是 prompt：

```markdown
---
name: debug
description: 调试当前会话
allowed-tools: [Read, Grep, Glob]
model: haiku
context: fork          # inline | fork
paths: ["**/*.ts"]     # 条件激活（gitignore 风格）
user-invocable: true
hooks:
  PreToolUse: ...
---

# Debug Skill
...prompt body...
```

**frontmatter 完整字段**：`name` / `description` / `when_to_use` / `user-invocable` / `allowed-tools` / `arguments` / `argument-hint` / `version` / `model` / `context` / `agent` / `effort` / `hooks` / `paths` / `shell` / `disable-model-invocation`

### lotus-app（当前）

多文件 TOML + Markdown 分离：

```
plugin-name/
├── plugin.toml        # 配置：id、keywords、model preference、display 等
├── workflow.toml      # 多步骤定义（可选）
└── prompts/
    ├── base.md        # 插件基础 prompt
    ├── step0.md       # 各步骤 prompt
    └── extract/       # 步骤间 checkpoint 提取提示
```

> **⚠️ 长期计划**：后期将迁移为与 claude-code-best 一致的 SKILL.md 单文件格式。

---

## 二、加载机制

| 维度 | claude-code-best | lotus-app |
|------|-----------------|-----------|
| **加载时机** | `getSkillDirCommands()` 首次调用时 memoize，按 cwd 缓存 | 应用启动时 `scan_external_plugins()` 一次性扫描 `src-tauri/plugins/` |
| **加载路径层级** | 4 级优先级：managed（`~/.claude/managed-skills/`）> user（`~/.config/claude/skills/`）> project（`.claude/skills/`）> legacy commands | 仅两个目录：`src-tauri/plugins/`（内置）+ `~/.renlijia/skills/`（用户安装） |
| **动态发现** | 文件操作时触发 `discoverSkillDirsForPaths()`，后台加载嵌套 `.claude/skills/` 目录 | 无动态发现，仅启动时一次性扫描 |
| **热重载** | chokidar 监听，文件变更 debounce 300ms → `clearSkillCaches()` 清空全部 memoize 缓存 | `reload_skill()` 命令 → unregister + re-register 单个 skill；notify 库文件监听 |
| **安装格式** | 无打包格式，直接目录复制 | `.aijia-skill` zip 包，含 plugin.toml + prompts/ + workflow.toml |
| **市场下载** | `fetchMcpSkillsForClient()` 仅预留 stub（未实现） | **完整实现**：云端 API + 下载 `.aijia-skill` zip → 解压到 `~/.renlijia/skills/` |

---

## 三、Skill 激活机制

### claude-code-best

LLM **主动调用** `SkillTool`，skill 列表预注入 system prompt，LLM 自主决策：

```
queryLoop()
  → 每次请求前 getSkills() 获取所有 skill
  → buildSystemPromptBlocks() 将 skill 列表编码进 system prompt（1% token 预算）
  → LLM 生成 → 调用 SkillTool(skill="debug", args="...")
    → executeForkedSkill() 或 processPromptSlashCommand()（inline）
```

**条件激活**：frontmatter `paths:` 字段，文件操作触发 `activateConditionalSkillsForPaths()`，gitignore 风格匹配。匹配后从 `conditionalSkills` Map 移入 `dynamicSkills` Map。

### lotus-app

**规则引擎关键词匹配**，代码逻辑触发，非 LLM 决策：

```
detect_activation(message, has_files, current_skill)
  → 遍历 SkillRegistry
  → should_activate(): keyword.contains(message) 大小写不敏感
  → 仅在 current_skill == "daily-assistant" 时可激活
  → 返回优先级最高的 skill id
```

**⚠️ 关键缺陷**：`detect_activation()` **未被 runtime 层调用**。全局搜索仅在定义处出现，`QueryEngine` / `SessionRuntime` / `ChatTurnDriver` 均无调用点。Skill 系统与 LLM 执行链路完全断开。

---

## 四、Skill 执行方式

| 维度 | claude-code-best | lotus-app |
|------|-----------------|-----------|
| **执行模式** | `inline`（展开进当前会话）或 `fork`（独立子 Agent 沙箱） | 同会话内切换 system_prompt，无 fork/沙箱 |
| **上下文隔离** | fork 模式：`createAgentId()` + `runAgent()` 独立上下文，独立 token budget | 无隔离，skill system_prompt 替换当前会话 prompt |
| **多步骤支持** | **无框架支持**，workflow 全靠 markdown 手写 | `WorkflowDefinition` + `WorkflowStep` 框架原生支持，step 间自动推进 |
| **system_prompt 构建** | skill 内容作为工具调用结果注入对话（inline），或作为子 agent 的初始 prompt（fork） | `system_prompt()` 返回：[app_base] + [plugin_base] + [step_prompt] + [tool限制] + [日期] |
| **mid-conversation 切换** | SkillTool 随时可被调用，任意轮次切换 | `TurnConfig` 初始化时快照锁定，不支持中途切换 |

---

## 五、工具白名单

### lotus-app：`DAILY_ALLOWED_TOOLS`

日常模式（DailyAssistantSkill）允许的工具（8 个）：

```rust
pub const DAILY_ALLOWED_TOOLS: &[&str] = &[
    "bash",
    "read_workspace_file",
    "write_file",
    "edit_file",
    "list_directory",
    "search_files",
    "get_file_info",
    "grep_content",
];
```

DeclarativeSkill 的工具限制通过 `workflow.toml` 的 `tools_only` 字段按步骤配置，`allowed_tool_names()` 返回当前步骤的白名单。

**⚠️ 关键缺陷**：`TurnConfig.allowed_tools` 初始化为 `None`，工具白名单**完全不生效**。

### claude-code-best：`allowedTools`

每个 skill 在注册时指定 `allowedTools`：

```typescript
registerBundledSkill({
  name: 'debug',
  allowedTools: ['Read', 'Grep', 'Glob'],  // 空数组 = 使用默认全集
  ...
})
```

**执行强制**：
- `inline` skill：通过 `contextModifier` 更新 `toolPermissionContext.alwaysAllowRules.session`
- `fork` skill：通过 `runAgent()` 的 `allowedTools` 参数，写入子 agent 的 session 级权限规则

---

## 六、内置 Skill 对比

| 项目 | 数量 | 类型 | 激活语言 |
|------|------|------|---------|
| **claude-code-best** | 16 个 bundled | 通用研发工具（loop/batch/debug/simplify/remember 等） | 英文 slash 命令为主 |
| **lotus-app** | 23 个 TOML + 1 Rust 硬编码（DailyAssistantSkill）| 垂直业务场景（HR/财务/销售/法务等） | 中文关键词匹配 |

**DailyAssistantSkill**：Rust 硬编码，`should_activate()` 永远返回 `false`（fallback 兜底），system_prompt 从 `prompts::get_system_prompt()` 动态加载，max_iterations=10，token_budget=4096。

---

## 七、核心差距汇总

### 执行链路断开（最严重）

| 问题 | 影响 | 断点位置 |
|------|------|---------|
| `detect_activation()` 未被 runtime 调用 | skill 永远不会自动切换 | `QueryEngine` / `ChatTurnDriver` 无调用 |
| `TurnConfig.allowed_tools` 为 `None` | skill 的工具白名单完全失效 | `executor.build_system_prompt()` 未注入 skill |
| 无 skill system_prompt 注入 | LLM 无法感知当前 skill 上下文 | `TauriLegacyTurnExecutor` 未接入 SkillRegistry |

### 架构模型差异

| 维度 | claude-code-best | lotus-app 现状 |
|------|-----------------|---------------|
| skill 触发者 | LLM 自主决策 | 代码规则引擎 |
| 执行隔离 | fork 子 agent 沙箱 | 无隔离 |
| 格式统一 | 单 SKILL.md | TOML + 多 MD |
| 工具限制实效 | 有效，双路强制 | 无效 |
| 市场完整度 | stub 未实现 | 完整实现 |
| 多步骤 workflow | 无框架支持 | 框架完整支持 |

---

## 八、修复优先级

| 优先级 | 修复项 | 改动位置 |
|--------|--------|---------|
| P0 | 在 `ChatTurnDriver::run_chat_turn()` 中调用 `detect_activation()` | `runtime/session_runtime.rs` |
| P0 | 将激活 skill 的 `system_prompt()` 注入 `TurnConfig` | `TauriLegacyTurnExecutor` |
| P0 | 将 `allowed_tool_names()` 绑到 `TurnConfig.allowed_tools` | `executor::build_system_prompt()` |
| P1 | 支持 mid-conversation skill 切换（重新构建 TurnConfig） | `ChatTurnDriver` |
| P2 | Skill 格式迁移为 SKILL.md（长期） | `plugin/declarative_skill.rs` + 23 个插件 |
