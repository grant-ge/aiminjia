# AIjia Skill Specification

**Date:** 2026-04-28
**Status:** Draft (pending user approval)
**Owner:** AIjia runtime team
**Reference:** `claude-code-best` skill subsystem (`/Users/a20250311/github/claude-code-best`)
**Predecessor (deprecated):** `plugin.toml + workflow.toml + prompts/step*.md` 形式的 AIjia 自创 skill 格式

---

## 1. 目标

把 AIjia 的 skill 系统**完全对齐** Claude Code 的 SKILL.md 设计，废弃自创的 stateful workflow pipeline，让 LLM 通过无状态 `load_skill` 工具自主拉起 skill 指令。

**核心原则**：

1. **磁盘唯一**：runtime 只从磁盘加载 skill；不再注册任何 builtin Rust skill 实现。
2. **无状态加载**：`load_skill` 是一个工具调用，返回 SKILL.md body 给 LLM；不改 system prompt、不限制工具、不持久化任何 active skill state。
3. **完全对齐 Claude Code**：frontmatter 字段、变量替换、catalog 注入策略、token 预算、`inline` / `fork` 两种执行模式都照搬 CC，仅替换变量前缀为 `${AIJIA_*}`。
4. **不需要兼容**：`plugin.toml` / `workflow.toml` / `is_analysis` / `switch_skill` / `SkillSessionStore` / `SkillRuntimePatch` / 全部前端 stateful 入口一次性删除。

---

## 2. Skill 磁盘形态

### 2.1 目录结构

```
<scan-root>/<skill-id>/
├── SKILL.md                  # 必需，单文件入口
├── scripts/                  # 可选，运行时脚本（LLM 通过 Read/Bash 自取）
├── references/               # 可选，运行时按需读取的参考资料
├── assets/                   # 可选，输出用资源（模板、图标等）
├── mcp-config.json           # 可选，AIjia 扩展：MCP 服务声明，运行时由链路注入 url
├── .skillignore              # 可选，AIjia 扩展：打包时排除规则
└── .env.example              # 可选，AIjia 扩展：环境变量样例（实际值不存）
```

**强制约束**：

- 必须是目录形态。仓库根下的孤立 `*.md` 文件**不会**被识别为 skill（与 CC 一致，见 `loadSkillsDir.ts:424-426`）。
- 必须包含字面量名为 `SKILL.md` 的入口文件（大小写敏感）。其它名字（如 `Skill.md`、`SKILL.MD`）一律忽略。
- skill-id（即外层目录名）必须匹配正则 `^[a-z0-9][a-z0-9_-]*$`，长度 ≤ 64。
- `scripts/` `references/` `assets/` 都是约定目录，runtime **不做任何特殊处理**。LLM 通过 `${AIJIA_SKILL_DIR}/scripts/foo.py` 这类路径自己读取。

### 2.2 SKILL.md 文件格式

```markdown
---
name: salary-query                  # 必需。skill 显示名，可与目录名不同。
description: >                      # 必需。LLM 看到的"何时用本 skill"的描述。
  薪酬市场数据查询助手——通过 MCP 工具查询中国市场薪酬数据。
  当用户询问某岗位薪酬、跳槽报价是否合理、团队人力成本估算时触发。
when_to_use: 用户提到薪酬/工资/薪资/跳槽报价  # 可选。会附加到 description 后展示给 LLM。
allowed-tools:                      # 可选。本 skill body 内执行 !`cmd` 时的预批工具列表。
  - bash
  - read_file
argument-hint: <city> <role>        # 可选。slash 命令补全提示。
arguments: city role years          # 可选。空格分隔或 YAML 列表，给 $<name> 替换用。
model: opus                         # 可选。skill 执行时的模型偏好。
effort: high                        # 可选。low|medium|high|max 或整数。
context: inline                     # 可选。inline（默认）| fork。
agent: code-reviewer                # 可选。当 context=fork 时使用的子代理名。
user-invocable: true                # 可选，默认 true。false = 仅 model-only。
disable-model-invocation: false     # 可选，默认 false。true = 仅 user / 命令触发。
version: "1.0"                      # 可选。仅信息性，runtime 不强校验。
paths:                              # 可选。glob 模式列表。匹配的文件被改动时才点亮 skill。
  - "src/**/*.ts"
hooks:                              # 可选。预留字段，本期解析后忽略。
shell: bash                         # 可选。bash（默认）| powershell。
metadata:                           # 可选。AIjia 扩展。
  label: 薪酬市场数据查询助手        # UI 卡片标题。
---

# Skill body（Markdown）

正文写给 LLM 的指令。可以使用以下变量占位符（执行时由 runtime 替换）：

- `${AIJIA_SKILL_DIR}` — 本 skill 的绝对目录路径。
- `${AIJIA_SESSION_ID}` — 当前会话 ID。
- `$ARGUMENTS` — 调用时透传的完整参数字符串。
- `$1`, `$2`, …, `$N` — 第 N 个 shell-parsed 参数（0-indexed 的别名 `$ARGUMENTS[N]` 也支持）。
- `$<name>` — 命名参数（取自 frontmatter `arguments:` 列表）。

可以使用 inline shell 块（执行结果在调用时被插入到 body 中）：

```
!`bash ${AIJIA_SKILL_DIR}/scripts/init.sh`
```

正文末尾如果存在变量未匹配，runtime 不报错，原样保留。
```

### 2.3 字段实现规划

| Field | Phase 1 行为 | 备注 |
|---|---|---|
| `name` | 解析 + 强制要求非空 | 同时是 catalog 中显示给 LLM 的 id |
| `description` | 解析 + 强制要求非空 | catalog 截断到 250 字符 |
| `when_to_use` | 解析 + 拼接到 description 后注入 catalog | 可选 |
| `allowed-tools` | 解析 + 在 inline 执行时把列表合入当前 turn 的 `alwaysAllowRules` | 用于 `!`cmd`` 块 |
| `argument-hint` | 解析 + 透传到前端 slash 补全 UI | 后端不消费 |
| `arguments` | 解析 + 用于变量替换 | 见 §4.1 |
| `model` | 解析 + 在 inline 执行时覆盖当前 turn 的 model 偏好 | 必须经 `ModelManager` 校验 |
| `effort` | 解析 + 在 inline 执行时覆盖 effort | 同上 |
| `context` | 解析 + `inline` 或 `fork` | 见 §5 |
| `agent` | 解析 + 仅 fork 模式生效 | 必须能在 `AgentRegistry` 中找到 |
| `user-invocable` | 解析 + UI 不展示该 skill 时禁用对应卡片/slash | catalog 仍发给 LLM |
| `disable-model-invocation` | 解析 + 从 catalog 中剔除 | 仅 user 触发 |
| `version` | 解析 + 仅记录到 catalog 元数据 | runtime 不消费 |
| `paths` | **本期解析后忽略** | Phase 2 接入文件 watcher 时启用 |
| `hooks` | **本期解析后忽略** | 后续接入 hooks pipeline 时启用 |
| `shell` | **本期解析后忽略** | 默认 bash；powershell 后续支持 |
| `metadata.label` | 解析 + 透传给前端 UI | AIjia 扩展，不在 CC 中 |

> **未实现字段（`paths`、`hooks`、`shell`）**：解析时只验证类型合法（YAML 结构正确），值原样存到 `Skill::raw_frontmatter`，运行时不消费。这样磁盘上的 skill 可以提前写好这些字段，等 runtime 实现时无缝启用。**未知字段**：忽略并记 `info!` 日志，不报错。

---

## 3. Skill 来源与扫描

### 3.1 扫描根目录（按优先级降序）

| 优先级 | 路径 | 代码入口 | 说明 |
|---|---|---|---|
| 1（高） | `~/.renlijia/users/t_{tenant}__u_{user}/skills/` | `UserScopedPaths::skills_dir()` | 当前登录用户的私有 skill；最高优先 |
| 2（低） | `~/.renlijia/skills/` | `AiJiaHome::skills_dir()` | 全局公共 skill；沿用现有路径，**不**新增 `global/skills/` |

**优先级语义**：当两个根下出现同名 `skill-id` 时，**用户级覆盖公共级**。被覆盖的 skill 不进入 catalog，也不可被 `load_skill` 加载（即用户故意"shadow"）。

> 不再扫描 `src-tauri/plugins/`（旧 builtin 目录）。该目录本期保留在仓库内但 runtime 不再读，供后续手动按新规范重写并放入上述两个根目录。

### 3.2 扫描时机

- **应用启动时**：扫描两个根目录，构建初始 `SkillRegistry`。
- **用户切换时**：清空用户级 catalog，重新扫描新用户的 `users/.../skills/`。公共级保持不变。
- **文件变更时**：通过 file watcher 监听两个根目录的新增/删除/修改事件，触发增量 reload（对应 CC 的 `skillChangeDetector`）。
- **手动触发**：暴露一个 Tauri command（如 `skills_reload`）供前端"重新扫描"按钮调用。

### 3.3 扫描算法（与 CC 行为对齐）

1. 对每个扫描根目录，列出一级子目录。
2. 跳过下列条目：
   - 名称以 `_` 或 `.` 开头（保留给 drafts、隐藏目录）
   - 不是目录（symlink 解析后再判断）
   - 缺少 `SKILL.md` 文件
3. 对每个有效目录，读取并解析 `SKILL.md`：
   - 解析 YAML frontmatter，验证必需字段
   - 校验 skill-id（与目录名匹配，且符合命名规则）
   - 缓存 body 文本（含变量占位符未替换的原文）
4. 用 `realpath()` 解析符号链接，按"实际路径"去重——同一个 skill 通过 symlink 出现两次只算一份。
5. 同名（skill-id）冲突时按 §3.1 优先级裁决，被覆盖的 skill 记 `warn!` 日志。

### 3.4 加载错误处理

- **frontmatter 解析失败**：跳过该 skill，记 `error!` 日志，不阻断扫描其它 skill。
- **必需字段缺失**：同上。
- **skill-id 命名不合法**：同上。

> 解析错误绝不能让 runtime 启动失败。失败的 skill 仅在管理 UI 中显式展示"加载失败"+原因。

---

## 4. 变量替换

### 4.1 变量表

执行 inline 模式（见 §5.1）时，runtime 在把 body 注入消息历史前**按以下顺序**做替换：

| 占位符 | 替换为 | 来源 |
|---|---|---|
| `$<name>` | frontmatter `arguments:` 列表中按顺序匹配的位置参数 | 调用方的 args 字符串 + shell parsing |
| `$ARGUMENTS[N]` | 第 N 个 shell-parsed 参数（0-indexed） | 同上 |
| `$N` (N=1..9) | `$ARGUMENTS[N-1]` 的简写 | 同上 |
| `$ARGUMENTS` | 调用方传入的原始 args 字符串 | `load_skill` tool 输入 |
| `${AIJIA_SKILL_DIR}` | skill 所在目录的绝对路径 | runtime 解析 |
| `${AIJIA_SESSION_ID}` | 当前 SessionId 字符串 | runtime 解析 |

**没有 `$ARGUMENTS` 占位符且 args 非空时**：runtime 在 body 末尾追加 `\n\nARGUMENTS: <args>` 一行（与 CC 一致）。

### 4.2 Inline shell 块

body 中的 `` !`cmd` `` 或三反引号代码块标记为 `!`：runtime 在变量替换之后、注入消息之前同步执行 shell 命令，把 stdout 替换原占位符。

```
!`echo hello`        →  hello
!`bash ${AIJIA_SKILL_DIR}/scripts/init.sh`   →  脚本输出
```

执行约束：

- 走当前 turn 的 permission pipeline。命令在 `allowed-tools` 中预批 → 自动通过；否则按正常 ask/deny 流程。
- 失败（非零退出码）→ 整次 `load_skill` 调用失败，返回 `ToolError::ExecutionFailed`，错误信息包含命令和 stderr。
- 超时：单条命令 30s，整段 SKILL.md 总执行时间 ≤ 120s。

> **MCP-loaded skill 跳过 shell 执行**（与 CC `loadSkillsDir.ts:374` 一致）。本期 AIjia 暂不支持 MCP-loaded skill，留接口。

---

## 5. Skill 执行模式

### 5.1 inline（默认）

当 `context: inline`（或未指定）时：

1. LLM 调用 `load_skill({ skill_id, args? })`。
2. runtime：
   - 在 `SkillRegistry` 中查找 `skill_id`，未找到 → 错误结果，列出可用 skill。
   - 读取 body，做变量替换 + shell 块执行（§4）。
   - 把展开后的 body 拼装为 tool result 的 `content`：
     ```
     ## <display-name> (<skill-id>)
     <Base directory: /abs/path/to/skill>
     <展开后的 body 全文>
     ```
   - 返回给 LLM。
3. 副作用（与 CC 一致）：
   - 把 `allowed-tools` 列表合入当前 turn 的 `alwaysAllowRules.command`。
   - 把 `model` / `effort` 覆盖到当前 turn config。
   - 把本次调用追加到 in-memory `STATE.invokedSkills` Map（用于 compact 后重注入）。
4. **不持久化任何 skill state**。会话切换、应用重启后，invokedSkills 不恢复（CC 行为一致）。

### 5.2 fork

当 `context: fork` 时：

1. LLM 调用 `load_skill`，runtime 通过 `agent` 字段（缺失则用默认 worker agent）spawn 一个子代理。
2. 子代理获得：
   - 父 turn 的 SkillRegistry（透传，子代理也能再 `load_skill`）。
   - 独立的 `RequestScopedRuntimeDeps`（`skill_sessions` 仍为 None——无 stateful workflow）。
   - 经过变量替换 + shell 块执行的 body，作为子代理的 system prompt。
3. 子代理在独立上下文执行至 `AgentDone`，把最终输出聚合成单段文本。
4. runtime 把聚合结果作为 tool result 返回给父 turn 的 LLM：
   ```
   ## Skill "<display-name>" completed (forked execution)
   Result:
   <子代理输出>
   ```

> fork 模式与父 turn 的 token / iteration / model 不共享。子代理用 skill frontmatter 中声明的 `model` / `effort`；未声明时继承父 turn。

### 5.3 工具签名

`load_skill` RuntimeTool 的 schema：

```json
{
  "type": "object",
  "properties": {
    "skill_id": {
      "type": "string",
      "description": "技能 ID，必须来自 catalog 中 `<system-reminder>` 列表"
    },
    "args": {
      "type": "string",
      "description": "可选，作为 $ARGUMENTS 透传给 SKILL.md"
    }
  },
  "required": ["skill_id"]
}
```

工具描述（Tool description，每次构造时由 runtime 拼接）：

> 加载一个专项技能并执行。当用户需求匹配 catalog 中某个 skill 时调用。无状态：不改系统提示、不限制后续工具、不持久化。可用 skill_id：`<列表>`。

**错误约定**：

| 输入 | 错误 |
|---|---|
| `{}`、`{"skill_id":""}`、`{"skill_id":"   "}` | `Missing required field: skill_id` |
| 未知 `skill_id` | `Unknown or unavailable skill: <id>. Available: <list>` |
| body shell 执行失败 | `Skill body shell command failed: <stderr>` |
| body 变量替换循环 | `Skill body variable substitution loop detected`（理论不会发生） |

---

## 6. Catalog 注入

### 6.1 注入形态

每轮 LLM 调用前，runtime 构造一段 `<system-reminder>` user message，内容形如：

```
<system-reminder>
The following skills are available for use with the load_skill tool:

- `salary-query` — 薪酬市场数据查询助手。当用户询问薪酬/跳槽报价合理性时触发。
- `biz-writing` — 商务写作助手。当用户需要写邮件、报告、备忘录时触发。
- `comp-analysis-v2` — 薪酬公平性诊断。当用户上传薪资表并要求做公平性分析时触发。

Use load_skill({ skill_id: "<id>" }) to load detailed instructions for any of these.
</system-reminder>
```

> 与 CC `wrapMessagesInSystemReminder` + `getSkillListingAttachments` 完全对齐，唯一差异是工具名 `Skill` → `load_skill`。

### 6.2 增量发送（sentSkillNames）

- runtime 维护内存 Map：`sent_skill_names: HashMap<AgentId, HashSet<SkillId>>`，按 agent 分桶。
- 每轮调用前对比当前 catalog 与已发送集合，**只发新增的 skill**。第一次调用全量；后续仅发本会话尚未见过的。
- compact 触发后**不重置**该 Map（与 CC `compact.ts:526` 注释一致），改为通过 `STATE.invokedSkills` 的重注入恢复 body 内容。
- skill 重新扫描（§3.2）后调用 `reset_sent_skill_names()` 强制下一轮全量发送。
- 会话 resume 时调用 `suppress_next_skill_listing()`，让首次发送跳过——transcript 已含历史。

### 6.3 Token 预算

- **总预算**：`context_window_tokens × 0.01`（默认按 4 chars/token 估算 → 200K 模型 ≈ 8000 字符）。
- **每条上限**：`MAX_LISTING_DESC_CHARS = 250` 字符。
- **降级策略**（与 CC `prompt.ts:70` 一致）：
  1. 满预算：每条都给 `description` + `when_to_use`，截断到 250 字符。
  2. 紧预算：每条只给 `name + 50 字符短描述`。
  3. 极紧预算：仅 `name` 列表。
- bundled / 系统级 skill 不参与降级（永远满描述）。AIjia Phase 1 没有 bundled skill，全部为磁盘 skill，按 user/global 来源不区分降级。

### 6.4 Compact 后重注入

- runtime 维护 `invoked_skills: HashMap<(AgentId, SkillId), InvokedSkillInfo { body, invoked_at, tokens }>`。
- 每次 inline 调用 `load_skill` 成功后写入。
- compact 触发时，按 `invoked_at` 倒序遍历，逐个把 body 重注入为 isMeta user message，直到达到每条 token 上限或总预算耗尽。
- fork 模式不写入此 Map（子代理上下文独立，结果已经聚合到父 tool result）。

---

## 7. 与现有 AIjia 架构的对接

### 7.1 模块划分

```
src-tauri/src/
├── plugin/
│   ├── skill/                # 新模块
│   │   ├── mod.rs
│   │   ├── loader.rs         # 扫描 + frontmatter 解析
│   │   ├── registry.rs       # SkillRegistry（替换现 plugin/registry.rs 中的 skill 部分）
│   │   ├── frontmatter.rs    # YAML 解析 + 字段验证
│   │   ├── substitution.rs   # 变量替换 + shell 块执行
│   │   └── invoked.rs        # invoked_skills + sent_skill_names
│   └── (旧 manifest.rs / declarative_skill.rs / builtin/skills/ 全部删除)
├── runtime/tools/builtin/
│   └── load_skill.rs         # 沿用现文件，重写 execute 实现
└── runtime/chat/
    └── (skill_session.rs 整文件删除；context_builder 删 precompute_result 参数)
```

### 7.2 路径解析

复用 `storage::UserScopedPathResolver` 和 `storage::AiJiaHome`：

```rust
let global_skills = home.skills_dir(); // ~/.renlijia/skills/
let user_skills = paths.skills_dir(); // ~/.renlijia/users/{scope}/skills/
```

### 7.3 Default skill 废弃

- `daily-assistant` 不再以 Skill trait 实现。
- 会话起手的 base prompt 改为代码内的常量（建议 `runtime::chat::base_prompt::DAILY_BASE_PROMPT`），通过 prompt 拼装器作为 system prompt 第 1 段。
- catalog 仍然由 `load_skill` 注入，没有 default skill 概念。
- 会话不再持久化 active skill。

### 7.4 子代理透传

- `SubAgentRuntimeDeps::skill_registry: Option<Arc<SkillRegistry>>` 由生产构造点透传。
- `SkillRegistry` 在子代理 dispatcher 中可用 → 子代理也能 `load_skill`。
- `skill_sessions` 字段从 `RequestScopedRuntimeDeps` 中删除（没有 stateful workflow 后无意义）。

### 7.5 Tauri command 清理

后端入口 `send_message` 移除 `selected_skill_id` / `selected_skill_label` 参数，前端 3 个入口（卡片 / slash / popover）随之改造。详见执行计划。

---

## 8. 不在本期实现的字段 / 能力

按用户决策（5A / 6B / 7A / 8A），这些已在 spec 中定义但 Phase 1 不消费：

- frontmatter 中的 `paths` glob 自动激活
- frontmatter 中的 `hooks`
- frontmatter 中的 `shell: powershell`
- MCP 来源的 skill（仅磁盘 skill）
- bundled（编译进二进制的）skill
- skill 内嵌资源 zip 提取（CC 的 `files` 字段）
- catalog 注入的 transcript-aware 过滤（CC 的 `--resume` 流程）

留空但解析时已校验类型合法的字段，未来启用时不需要 skill 作者重写 SKILL.md。

---

## 9. 安全与权限

- `allowed-tools` 是**预批列表**而非限制：声明的工具在该 skill 上下文里跳过 ask 提示；未声明的工具仍然走正常 permission pipeline。
- `!`cmd`` shell 块继承当前 turn 的 permission policy，且把 `allowed-tools` 合入 `alwaysAllowRules.command`。
- skill body 中如果意外包含 `<system-reminder>`、`<tool_use>` 等 fenced sentinel，runtime 在变量替换前**整体转义**，避免 prompt 注入。
- skill 来源仅限本机磁盘的两个目录（§3.1），用户级目录天然按登录用户隔离。
- 错误日志中不打印 SKILL.md body 内容，避免敏感信息泄漏（`mcp-config.json` 由链路注入凭据）。

---

## 10. 验证与测试约束

实现 PR 必须满足：

1. **frontmatter 解析覆盖率**：每个支持字段至少 1 个正向 + 1 个负向单测；未实现字段验证"解析后忽略不报错"。
2. **变量替换**：`${AIJIA_SKILL_DIR}` / `${AIJIA_SESSION_ID}` / `$ARGUMENTS` / `$1..$9` / `$<name>` 各 1 个测试；未匹配时原样保留。
3. **目录扫描**：用户级覆盖公共级 1 个测试；非法 skill-id 跳过 1 个；symlink 去重 1 个。
4. **catalog 增量**：初次全量 + 后续只发新增 + 重新扫描后强制全量 1 套测试。
5. **inline 执行**：成功 / 未知 skill_id / 缺失 skill_id / shell 失败 / fork 模式回收子代理结果各 1 个测试。
6. **后向不兼容性**：1 个 review 测试，断言 `plugin.toml` `workflow.toml` `step*.md` 文件**不被读取**（构造仅含这些文件的目录，断言 SkillRegistry 中不出现该 skill）。
7. **集成测试**：以 `~/.renlijia/users/<scope>/skills/salary-query` 为 fixture，端到端验证 LLM `load_skill("salary-query")` → tool result 含 base prompt 内容。

---

## 11. 实施切割（先 spec、后 plan）

实施计划文件：`docs/superpowers/plans/2026-04-28-aijia-skill-system-rewrite.md`（本 spec 通过后单独编写）。

预计 5 个 Phase：

- **Phase A**：删除 stateful workflow 链路（switch_skill / SkillRuntimePatch / SkillSessionStore / precompute / is_analysis）。
- **Phase B**：删除旧 skill 格式相关代码（manifest.rs / declarative_skill.rs / builtin/skills/ / scan_external_plugins）。
- **Phase C**：实现 §2–§4：SKILL.md 解析 + 双根扫描 + 变量替换 + shell 执行。
- **Phase D**：实现 §5–§6：load_skill inline + fork + catalog 增量注入。
- **Phase E**：前端 3 个 stateful 入口拆除 + Tauri command 清理 + 测试与文档。

每个 Phase 独立可 review；A、B 可并行。

---

## 12. 决策记录

- **2026-04-28** 5A／6B／7A／8A／9B（修订）／10B／11—UI 已完成：完全对齐 CC 16 字段、inline+fork、1% 预算 + 250 字符上限、本期 runtime 不读旧 plugins 目录但保留文件、公共目录沿用现有 `~/.renlijia/skills/`、用户级目录 `~/.renlijia/users/{scope}/skills/`、import-skill UI 已就绪。
- **2026-04-28** 不需要兼容旧格式：daily-assistant builtin、plugin.toml 解析、workflow pipeline、stateful switch_skill 一次性切除。
