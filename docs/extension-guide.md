# 扩展指南 — AI小家插件开发

> 增加新能力时，优先通过插件系统扩展，而非修改核心代码。

---

## 目录

1. [架构概览](#1-架构概览)
2. [扩展类型速查](#2-扩展类型速查)
3. [添加 Tool（工具）](#3-添加-tool工具)
4. [添加 Skill（垂直场景）](#4-添加-skill垂直场景)
5. [plugin.toml 规范](#5-plugintoml-规范)
6. [workflow.toml 规范](#6-workflowtoml-规范)
7. [开发与调试](#7-开发与调试)
8. [打包与分发](#8-打包与分发)
9. [设计原则](#9-设计原则)

---

## 1. 架构概览

AI小家 的插件系统基于 **Tool + Skill** 两层抽象：

```
用户消息
    ↓
┌──────────────────────────────────────────────┐
│            Core Engine（核心引擎）              │
│                                               │
│  agent_loop：参数化执行，不含业务逻辑            │
│                                               │
│  1. 哪个 Skill 激活？  → SkillRegistry 检测    │
│  2. 用什么 prompt？    → 活跃 Skill 提供       │
│  3. 用哪些工具？       → ToolRegistry 按 Skill │
│                         过滤后提供             │
│  4. 什么时候要确认？   → Skill 的 workflow 定义 │
├──────────────────────────────────────────────┤
│  ┌──────────────┐     ┌───────────────────┐  │
│  │ ToolRegistry │     │  SkillRegistry    │  │
│  │              │     │                   │  │
│  │ 内置 10 个    │     │ daily-assistant   │  │
│  │ + 插件扩展   │     │ comp-analysis     │  │
│  │              │     │ + 插件扩展        │  │
│  └──────────────┘     └───────────────────┘  │
└──────────────────────────────────────────────┘
```

**Tool**：原子操作（搜索、执行代码、生成报告等），LLM 通过 tool_use 调用。

**Skill**：垂直场景能力包（prompt + 工具过滤 + 可选工作流），告诉核心引擎"如何处理一类任务"。

---

## 2. 扩展类型速查

| 我想要… | 扩展方式 | 需要写 Rust？ | 复杂度 |
|---------|---------|:---:|:---:|
| 加一个新工具（LLM 可调用） | Rust 内置 Tool | 是 | ★★ |
| 加一个新工具（Python 脚本） | Python Tool 插件 | 否 | ★ |
| 加一个新垂直场景（多步工作流） | 声明式 Skill 插件 | 否 | ★★ |
| 加一个新垂直场景（复杂逻辑） | Rust 内置 Skill | 是 | ★★★ |
| 修改现有分析流程的 prompt | 编辑 Skill 的 prompt 文件 | 否 | ★ |
| 修改现有分析流程的步骤/工具 | 编辑 Skill 代码或配置 | 视情况 | ★★ |

---

## 3. 添加 Tool（工具）

### 3.1 方式一：Rust 内置 Tool

适用于需要调用 Rust 生态库或系统 API 的工具。

**步骤：**

1. 在 `src-tauri/src/plugin/builtin/tools/` 新建文件

```rust
// src-tauri/src/plugin/builtin/tools/my_tool.rs
use async_trait::async_trait;
use serde_json::{json, Value};
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct MyTool;

#[async_trait]
impl ToolPlugin for MyTool {
    fn name(&self) -> &str { "my_tool" }

    fn description(&self) -> &str {
        "Brief description for LLM to understand when to use this tool."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "What to do"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        let query = input["query"].as_str()
            .ok_or_else(|| ToolError::MissingArgument("query".into()))?;

        // 业务逻辑...
        // ctx.storage — 访问数据存储
        // ctx.file_manager — 文件管理
        // ctx.workspace_path — 工作目录路径
        // ctx.conversation_id — 当前会话 ID

        Ok(ToolOutput::success(format!("Result: {}", query)))
    }
}
```

2. 在 `builtin/tools/mod.rs` 注册

```rust
pub mod my_tool;

pub async fn register_builtin_tools(registry: &ToolRegistry) {
    let tools: Vec<Arc<dyn crate::plugin::ToolPlugin>> = vec![
        // ... 现有工具 ...
        Arc::new(my_tool::MyTool),
    ];
    for tool in tools {
        registry.register(tool, "builtin").await;
    }
}
```

3. 如果工具仅在特定 Skill 的特定步骤可用，在对应 Skill 的 `tool_filter()` 中配置

### 3.2 方式二：Python Tool 插件（无需写 Rust）

适用于数据处理、文件解析等 Python 生态擅长的任务。

**目录结构：**

```
src-tauri/plugins/resume-parser/
├── plugin.toml          # 插件清单
└── handler.py           # Python 处理器
```

**plugin.toml：**

```toml
[plugin]
id = "resume-parser"
name = "简历解析器"
type = "tool"
runtime = "python"
handler = "handler.py"
```

**handler.py：**

```python
def schema():
    """返回工具的 JSON Schema 定义，LLM 用此理解工具用途和参数。"""
    return {
        "name": "resume_parser",
        "description": "Parse resume files and extract structured data",
        "input_schema": {
            "type": "object",
            "properties": {
                "file_id": {
                    "type": "string",
                    "description": "ID of the uploaded resume file"
                }
            },
            "required": ["file_id"]
        }
    }

def handle(args, context):
    """
    执行工具逻辑。

    Args:
        args: dict — LLM 传入的参数（符合 schema 定义）
        context: dict — 包含 workspace_path, conversation_id

    Returns:
        dict — 必须包含 content (str)，可选 is_error (bool), data (dict)
    """
    file_id = args["file_id"]
    workspace = context["workspace_path"]

    # 业务逻辑...

    return {
        "content": "解析结果...",
        "is_error": False,
        "data": {"name": "张三", "education": "本科"}
    }
```

**安全说明：** Python handler 的参数通过临时 JSON 文件传递（非字符串拼接），防止代码注入。

---

## 4. 添加 Skill（垂直场景）

### 4.1 方式一：声明式 Skill（TOML + Markdown，无需写 Rust）

适用于绝大多数新场景。只需配置触发关键词、工作流步骤、每步的 prompt 和工具。

**目录结构：**

```
src-tauri/plugins/recruit-analysis/
├── plugin.toml          # 插件清单
├── workflow.toml        # 工作流定义
├── base.md              # 基础 prompt（所有步骤共享）
└── prompts/
    ├── step1.md          # 步骤 1 专用 prompt
    ├── step2.md          # 步骤 2 专用 prompt
    └── step3.md          # 步骤 3 专用 prompt
```

**plugin.toml：**

```toml
[plugin]
id = "recruit-analysis"
name = "招聘分析"
type = "skill"

[trigger]
keywords = ["招聘分析", "简历筛选", "候选人评估", "recruiting"]
requires_files = true     # 需要用户上传文件才触发

[model]
preference = "deep_reasoning"  # 模型偏好（可选）

[defaults]
max_iterations = 15       # 每步最大迭代次数
token_budget = 8192       # LLM 输出 token 预算
```

**workflow.toml：**

```toml
[[steps]]
id = "step0"
name = "需求确认"
prompt = "prompts/step0.md"
tools_only = ["load_file", "save_analysis_note"]
max_iterations = 5
requires_confirmation = true

[[steps]]
id = "step1"
name = "简历数据分析"
prompt = "prompts/step1.md"
tools_only = ["load_file", "execute_python", "save_analysis_note", "update_progress"]
max_iterations = 15
requires_confirmation = true

[[steps]]
id = "step2"
name = "候选人匹配评估"
prompt = "prompts/step2.md"
tools_only = ["execute_python", "web_search", "save_analysis_note", "update_progress"]
max_iterations = 15
requires_confirmation = true

[[steps]]
id = "step3"
name = "评估报告"
prompt = "prompts/step3.md"
tools_only = ["execute_python", "generate_report", "generate_chart", "export_data"]
max_iterations = 15
requires_confirmation = true
```

**base.md（示例）：**

```markdown
你是一位专业的招聘分析顾问...

## 分析原则
- ...
```

**prompts/step1.md（示例）：**

```markdown
## Step 1: 简历数据分析

### 目标
分析上传的简历文件，提取关键信息...

### 输出要求
1. ...
2. ...

### 完成条件
- 调用 save_analysis_note 保存分析结论
```

**Prompt 组合规则：** 最终 system prompt = `base.md` + `\n\n` + `stepN.md`

### 4.2 方式二：Rust 内置 Skill

适用于需要复杂确认逻辑或自定义步骤流转的场景（如当前的薪酬分析 Skill）。

**步骤：**

1. 在 `src-tauri/src/plugin/builtin/skills/` 新建文件
2. 实现 `Skill` trait

```rust
use async_trait::async_trait;
use crate::plugin::skill_trait::*;

pub struct MyAnalysisSkill;

#[async_trait]
impl Skill for MyAnalysisSkill {
    fn id(&self) -> &str { "my-analysis" }
    fn display_name(&self) -> &str { "我的分析" }
    fn description(&self) -> &str { "描述..." }

    fn should_activate(&self, message: &str, has_files: bool, current_skill: &str) -> bool {
        // 仅在日常模式下触发
        if current_skill != "daily-assistant" { return false; }
        if !has_files { return false; }
        let lower = message.to_lowercase();
        ["关键词1", "关键词2"].iter().any(|kw| lower.contains(kw))
    }

    fn system_prompt(&self, state: &SkillState) -> String {
        match state.current_step.as_deref() {
            Some("step0") => "Step 0 prompt...".into(),
            Some("step1") => "Step 1 prompt...".into(),
            _ => "Base prompt...".into(),
        }
    }

    fn tool_filter(&self, state: &SkillState) -> ToolFilter {
        match state.current_step.as_deref() {
            Some("step0") => ToolFilter::Only(vec![
                "load_file".into(),
                "save_analysis_note".into(),
            ]),
            _ => ToolFilter::All,
        }
    }

    fn max_iterations(&self, state: &SkillState) -> usize {
        match state.current_step.as_deref() {
            Some("step0") => 5,
            _ => 15,
        }
    }

    fn token_budget(&self, _state: &SkillState) -> u32 { 8192 }

    fn workflow(&self) -> Option<WorkflowDefinition> {
        Some(WorkflowDefinition {
            initial_step: "step0".into(),
            steps: vec![
                WorkflowStep {
                    id: "step0".into(),
                    display_name: "需求确认".into(),
                    requires_confirmation: true,
                },
                WorkflowStep {
                    id: "step1".into(),
                    display_name: "数据分析".into(),
                    requires_confirmation: true,
                },
            ],
        })
    }

    fn on_step_complete(&self, state: &mut SkillState, user_message: &str) -> StepAction {
        // 自定义确认/取消/推进逻辑
        let lower = user_message.trim().to_lowercase();
        if ["取消", "算了", "cancel"].iter().any(|p| lower == *p) {
            return StepAction::Abort;
        }
        if ["确认", "继续", "ok", "yes"].iter().any(|p| lower == *p) {
            match state.current_step.as_deref() {
                Some("step0") => return StepAction::AdvanceToStep("step1".into()),
                Some("step1") => return StepAction::Finish,
                _ => {}
            }
        }
        StepAction::WaitForUser  // 用户有反馈，重跑当前步骤
    }
}
```

3. 在 `builtin/skills/mod.rs` 注册

```rust
pub mod my_analysis;

pub async fn register_builtin_skills(registry: &SkillRegistry) {
    // ... 现有注册 ...
    registry.register(Arc::new(my_analysis::MyAnalysisSkill), "builtin").await;
}
```

---

## 5. plugin.toml 规范

```toml
[plugin]
id = "unique-id"          # 必填，唯一标识（英文，kebab-case）
name = "显示名称"          # 必填，UI 显示名
type = "tool"              # 必填，"tool" 或 "skill"
runtime = "python"         # 可选，仅 type="tool" 时有效，目前支持 "python"
handler = "handler.py"     # 可选，runtime="python" 时必填

[trigger]                  # 可选，仅 type="skill" 时有效
keywords = ["关键词1", "keyword2"]  # 触发关键词（用户消息包含任一则匹配）
requires_files = false     # 是否需要用户上传了文件才触发

[model]                    # 可选
preference = "deep_reasoning"  # 模型偏好，可选值见下表

[defaults]                 # 可选
max_iterations = 10        # 默认最大迭代次数
token_budget = 4096        # 默认 token 预算

[capabilities]             # 可选（预留）
file_system = "workspace"  # 文件系统权限
```

**模型偏好 (`preference`) 可选值：**

| 值 | 含义 | 典型模型 |
|----|------|---------|
| `deep_reasoning` | 深度推理 | DeepSeek R1, Claude |
| `cost_efficient` | 低成本 | DeepSeek V3, Qwen |
| `long_context` | 长上下文 | 128K+ 模型 |
| `code_generation` | 代码生成 | DeepSeek V3, Claude |
| `instruction_following` | 指令遵循 | Claude, GPT-4 |

---

## 6. workflow.toml 规范

每个 `[[steps]]` 定义工作流的一个步骤：

```toml
[[steps]]
id = "step0"                        # 必填，步骤 ID（建议 stepN 格式）
name = "步骤显示名称"                 # 必填
prompt = "prompts/step0.md"          # 可选，步骤 prompt 文件路径（相对插件目录）
tools_only = ["tool1", "tool2"]      # 可选，仅允许这些工具（与 tools_exclude 互斥）
tools_exclude = ["tool3"]            # 可选，排除这些工具
max_iterations = 10                  # 可选，覆盖 [defaults] 的值
requires_confirmation = true         # 可选，默认 true，步骤完成后等待用户确认
```

**工具过滤优先级：** `tools_only` > `tools_exclude` > 全部可用

**步骤流转：** 步骤按 `[[steps]]` 数组顺序执行。用户确认后自动推进到下一步；用户取消则中止整个工作流。

---

## 7. 开发与调试

### 开发期快速验证

1. 将插件目录放在 `src-tauri/plugins/` 下
2. 运行 `pnpm tauri:dev`，应用启动时自动加载
3. 查看终端日志确认加载状态：
   ```
   [INFO] Loaded Python tool plugin: resume-parser
   [INFO] Loaded declarative skill plugin: recruit-analysis
   ```
4. 在设置面板的「插件管理」Tab 查看已注册的 Tools 和 Skills

### 调试 Python 工具

- Python handler 在沙箱子进程中执行，stdout 作为结果返回
- 错误信息（stderr）会显示在 LLM 对话中
- `handler.py` 修改后需重启应用生效

### 调试 Prompt

- 声明式 Skill 的 prompt `.md` 文件修改后需重启应用
- 内置 Skill 的 prompt 修改后需重新编译

### 常见问题

| 问题 | 原因 | 解决 |
|------|------|------|
| 插件未出现在列表 | `plugin.toml` 格式错误 | 检查日志中的 `Invalid plugin.toml` 警告 |
| Python 工具加载失败 | `handler.py` 的 `schema()` 报错 | 检查日志中的 `Failed to load schema` |
| Skill 未触发 | 关键词不匹配或 `requires_files` 不满足 | 检查 `[trigger]` 配置 |
| 步骤 prompt 为空 | prompt 文件路径错误 | 检查日志中的 `Prompt file not found` 警告 |
| 工具名称冲突 | 插件工具名与内置工具同名 | 改用不同名称（内置工具名受保护，不可被覆盖）|

---

## 8. 打包与分发

插件通过 Tauri 的 `bundle.resources` 机制随应用打包分发：

**目录结构：**

```
src-tauri/
├── plugins/                    # 所有扩展插件放在这里
│   ├── resume-parser/          # Python Tool 插件示例
│   │   ├── plugin.toml
│   │   └── handler.py
│   └── recruit-analysis/       # 声明式 Skill 插件示例
│       ├── plugin.toml
│       ├── workflow.toml
│       ├── base.md
│       └── prompts/
│           ├── step1.md
│           └── step2.md
├── tauri.conf.json             # 已配置 "plugins": "plugins"
└── ...
```

**tauri.conf.json 中的资源配置：**

```json
{
  "bundle": {
    "resources": {
      "python-runtime": "python-runtime",
      "prompts": "prompts",
      "plugins": "plugins"
    }
  }
}
```

构建后，`plugins/` 目录会被打包到应用资源目录中（macOS: `.app/Contents/Resources/plugins/`），应用启动时自动扫描加载。

---

## 9. 设计原则

### 优先扩展，而非修改核心

新增能力时，优先考虑：

1. **声明式 Skill 插件** — 只需 TOML + Markdown，零 Rust 代码
2. **Python Tool 插件** — 数据处理类工具，一个 `.py` 文件搞定
3. **Rust 内置 Tool** — 需要系统 API 或 Rust 生态库时
4. **Rust 内置 Skill** — 仅在需要复杂自定义流转逻辑时

### Trait 协议

**ToolPlugin trait（工具协议）：**

| 方法 | 说明 |
|------|------|
| `name()` | 唯一标识，LLM 用此调用工具 |
| `description()` | 简短描述，LLM 用此判断何时调用 |
| `input_schema()` | JSON Schema，LLM 用此构造参数 |
| `execute(ctx, input)` | 执行逻辑，返回 `ToolOutput` |

**Skill trait（场景协议）：**

| 方法 | 说明 | 默认值 |
|------|------|--------|
| `id()` | 唯一标识 | — |
| `display_name()` | UI 显示名 | — |
| `should_activate(msg, files, current)` | 是否激活此 Skill | — |
| `system_prompt(state)` | 返回系统提示词 | — |
| `tool_filter(state)` | 工具过滤规则 | — |
| `model_preference(state)` | 模型偏好 | `None` |
| `max_iterations(state)` | 最大迭代次数 | `10` |
| `token_budget(state)` | Token 预算 | `4096` |
| `workflow()` | 工作流定义 | `None`（自由对话） |
| `on_step_complete(state, msg)` | 步骤完成后决策 | `WaitForUser` |

### PluginContext（共享服务）

所有插件执行时都能访问：

| 字段 | 类型 | 用途 |
|------|------|------|
| `storage` | `Arc<AppStorage>` | 数据存储（会话、消息、设置、记忆） |
| `file_manager` | `Arc<FileManager>` | 文件上传/存储/清理 |
| `workspace_path` | `PathBuf` | 用户工作目录 |
| `conversation_id` | `String` | 当前会话 ID |
| `tavily_api_key` | `Option<String>` | 搜索 API Key |
| `app_handle` | `Option<AppHandle>` | Tauri 应用句柄（事件发送等） |

### 内置工具清单

| 工具名 | 用途 |
|--------|------|
| `web_search` | 联网搜索（Tavily / SearXNG） |
| `execute_python` | 沙箱 Python 代码执行 |
| `load_file` | 加载上传文件，数据自动注入 execute_python 环境 |
| `generate_report` | 生成 HTML 分析报告 |
| `generate_chart` | 生成 PNG 图表 |
| `hypothesis_test` | 统计假设检验 |
| `anomaly_detect` | 异常值检测 |
| `save_analysis_note` | 保存分析记录（跨步骤传递） |
| `export_data` | 导出 CSV/Excel/JSON |
| `update_progress` | 更新分析进度条 |

这些内置工具名受保护，外部插件不能使用相同名称。
