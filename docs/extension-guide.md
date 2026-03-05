# 扩展指南 — AI小家插件开发

增加功能时，**优先通过插件扩展**，避免修改核心引擎。

## 扩展类型速查

| 我想要… | 扩展方式 | 需要 Rust？ |
|---------|---------|:-----------:|
| 加工具（Python 脚本） | Python Tool 插件 | 否 |
| 加工具（需系统 API） | Rust 内置 Tool | 是 |
| 加垂直场景（多步工作流） | 声明式 Skill 插件 | 否 |
| 加垂直场景（复杂逻辑） | Rust 内置 Skill | 是 |
| 修改分析流程 prompt | 编辑 Skill 的 `.md` 文件 | 否 |

---

## 添加 Python Tool

```
src-tauri/plugins/resume-parser/
├── plugin.toml
└── handler.py
```

**plugin.toml**：

```toml
[plugin]
id = "resume-parser"
name = "简历解析器"
type = "tool"
runtime = "python"
handler = "handler.py"
```

**handler.py**：

```python
def schema():
    return {
        "name": "resume_parser",
        "description": "Parse resume files and extract structured data",
        "input_schema": {
            "type": "object",
            "properties": {
                "file_id": {"type": "string", "description": "ID of uploaded file"}
            },
            "required": ["file_id"]
        }
    }

def handle(args, context):
    # context: {"workspace_path": "...", "conversation_id": "..."}
    return {"content": "解析结果...", "is_error": False, "data": {...}}
```

---

## 添加 Rust Tool

1. 在 `plugin/builtin/tools/` 新建文件，实现 `ToolPlugin` trait
2. 在 `builtin/tools/mod.rs` 注册

```rust
#[async_trait]
impl ToolPlugin for MyTool {
    fn name(&self) -> &str { "my_tool" }
    fn description(&self) -> &str { "Brief description for LLM" }
    fn input_schema(&self) -> Value { json!({...}) }
    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::success("Result"))
    }
}
```

---

## 添加声明式 Skill

```
src-tauri/plugins/recruit-analysis/
├── plugin.toml
├── workflow.toml
├── base.md                # 基础 prompt（所有步骤共享）
└── prompts/
    ├── step0.md            # 步骤专用 prompt
    └── step1.md
```

**plugin.toml**：

```toml
[plugin]
id = "recruit-analysis"
name = "招聘分析"
type = "skill"

[trigger]
keywords = ["招聘分析", "候选人评估"]
requires_files = true

[defaults]
max_iterations = 15
token_budget = 8192
```

**workflow.toml**：

```toml
[[steps]]
id = "step0"
name = "需求确认"
prompt = "prompts/step0.md"
tools_only = ["load_file", "save_analysis_note"]
max_iterations = 5
requires_confirmation = true
```

**Prompt 组合**：最终 system prompt = `base.md` + `\n\n` + `stepN.md`

---

## 添加 Rust Skill

在 `plugin/builtin/skills/` 新建文件，实现 `Skill` trait，在 `mod.rs` 注册。

关键方法：`should_activate()`, `system_prompt()`, `tool_filter()`, `workflow()`, `on_step_complete()`

仅在需要复杂自定义流转逻辑时使用。

---

## 配置字段参考

**plugin.toml**：

```toml
[plugin]
id = "unique-id"          # 必填，kebab-case
name = "显示名称"          # 必填
type = "tool"              # "tool" | "skill"
runtime = "python"         # 可选，type="tool" 时
handler = "handler.py"     # runtime="python" 时必填

[trigger]                  # 仅 type="skill"
keywords = ["关键词"]
requires_files = false

[defaults]
max_iterations = 10
token_budget = 4096
```

**workflow.toml**：

```toml
[[steps]]
id = "step0"                        # 必填
name = "步骤名称"                     # 必填
prompt = "prompts/step0.md"          # 可选
tools_only = ["tool1", "tool2"]      # 可选（与 tools_exclude 互斥）
max_iterations = 10                  # 可选
requires_confirmation = true         # 可选，默认 true
```

---

## 开发与调试

1. 插件目录放在 `src-tauri/plugins/`
2. `pnpm tauri:dev` 启动时自动加载
3. 查看终端日志确认加载状态
4. `.md` 和 `.py` 文件修改后需重启应用

---

## 内置工具清单

| 工具名 | 用途 |
|--------|------|
| `web_search` | 联网搜索 |
| `execute_python` | Python 代码执行 |
| `load_file` | 加载上传文件 |
| `generate_report` | 生成报告 |
| `generate_chart` | 生成图表 |
| `hypothesis_test` | 统计假设检验 |
| `detect_anomalies` | 异常值检测 |
| `save_analysis_note` | 保存分析记录 |
| `export_data` | 导出数据 |
| `update_progress` | 更新进度条 |

内置工具名受保护，外部插件不能使用相同名称。
