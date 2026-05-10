//! ToolCatalog — 工具元数据的单一真相源。
//!
//! 所有工具（primitive/power/composite/support）都在此注册。
//! `llm/tools.rs` 中的旧 schema 定义降级为兼容层，不再新增。
//! `plugin/registry.rs` 的运行时注册以本 catalog 为权威来源。

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

use serde_json::{json, Value};

use crate::runtime::tools::definition::{ToolDefinition, ToolKind};

/// 完整工具目录条目（含 JSON Schema）。
#[derive(Clone, Debug)]
pub struct CatalogEntry {
    pub definition: ToolDefinition,
    /// LLM 调用时传递的 JSON Schema（参数定义）。
    pub json_schema: Value,
}

impl CatalogEntry {
    pub fn new(definition: ToolDefinition, json_schema: Value) -> Self {
        Self {
            definition,
            json_schema,
        }
    }
}

/// 工具目录。
pub struct ToolCatalog {
    entries: HashMap<String, CatalogEntry>,
}

impl ToolCatalog {
    /// 返回默认内置工具目录（全量）。
    pub fn default_catalog() -> Self {
        build_default_catalog()
    }

    /// 按 ID 查找工具定义。
    pub fn get(&self, id: &str) -> Option<&ToolDefinition> {
        self.entries.get(id).map(|e| &e.definition)
    }

    /// 按 ID 查找完整目录条目（含 JSON Schema）。
    pub fn get_entry(&self, id: &str) -> Option<&CatalogEntry> {
        self.entries.get(id)
    }

    /// 返回所有工具 ID。
    pub fn all_ids(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    /// 返回指定 kind 的所有工具定义。
    pub fn by_kind(&self, kind: &ToolKind) -> Vec<&ToolDefinition> {
        self.entries
            .values()
            .filter(|e| &e.definition.kind == kind)
            .map(|e| &e.definition)
            .collect()
    }

    fn insert(&mut self, entry: CatalogEntry) {
        self.entries.insert(entry.definition.id.clone(), entry);
    }
}

/// 运行时可变工具目录。
#[derive(Clone, Debug)]
pub struct DynamicToolCatalog {
    entries: Arc<RwLock<HashMap<String, CatalogEntry>>>,
}

impl DynamicToolCatalog {
    /// 返回带 builtin 默认项的动态 catalog。
    pub fn new_with_defaults() -> Self {
        let catalog = build_default_catalog();
        Self {
            entries: Arc::new(RwLock::new(catalog.entries)),
        }
    }

    /// 动态注册或更新一条工具目录记录。
    pub fn register_entry(&self, entry: CatalogEntry) {
        self.entries
            .write()
            .unwrap()
            .insert(entry.definition.id.clone(), entry);
    }

    /// 移除一条动态目录记录。
    ///
    /// 主要供 MCP 这类运行时注册工具在 disconnect / refresh 时清理使用。
    pub fn remove_entry(&self, id: &str) -> Option<CatalogEntry> {
        self.entries.write().unwrap().remove(id)
    }

    /// 按 ID 查找工具定义。
    pub fn get(&self, id: &str) -> Option<ToolDefinition> {
        self.entries
            .read()
            .unwrap()
            .get(id)
            .map(|entry| entry.definition.clone())
    }

    /// 按 ID 查找完整目录条目（含 JSON Schema）。
    pub fn get_entry(&self, id: &str) -> Option<CatalogEntry> {
        self.entries.read().unwrap().get(id).cloned()
    }

    /// 返回所有工具 ID。
    pub fn all_ids(&self) -> Vec<String> {
        self.entries.read().unwrap().keys().cloned().collect()
    }

    /// 返回指定 kind 的所有工具定义。
    pub fn by_kind(&self, kind: &ToolKind) -> Vec<ToolDefinition> {
        self.entries
            .read()
            .unwrap()
            .values()
            .filter(|entry| &entry.definition.kind == kind)
            .map(|entry| entry.definition.clone())
            .collect()
    }
}

fn build_default_catalog() -> ToolCatalog {
    let mut c = ToolCatalog {
        entries: HashMap::new(),
    };

    // ── Primitive: workspace tools ──────────────────────────────────
    c.insert(CatalogEntry::new(
        ToolDefinition::new("Read", "读取授权工作目录中的文本文件内容")
            .with_kind(ToolKind::Primitive)
            .with_read_only(true)
            .with_max_result_size_chars(16_000)
            .with_capability_scope(["workspace:read"]),
        json!({
            "type": "object",
            "required": ["file_path"],
            "properties": {
                "file_path": { "type": "string", "description": "文件路径（绝对路径或相对授权工作目录的路径）" },
                "offset": { "type": "integer", "description": "起始行号（1-based）。指定 offset 时按行读取而非字节" },
                "limit": { "type": "integer", "description": "最多读取行数。配合 offset 使用，默认 2000 行" },
                "max_bytes": { "type": "integer", "description": "字节模式上限（不指定 offset/limit 时生效）。默认 1048576", "default": 1048576 }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new("Glob", "在授权工作目录中搜索匹配 glob 模式的文件")
            .with_kind(ToolKind::Primitive)
            .with_read_only(true)
            .with_max_result_size_chars(4_000)
            .with_capability_scope(["workspace:read"]),
        json!({
            "type": "object",
            "required": ["pattern"],
            "properties": {
                "pattern": { "type": "string", "description": "文件名 glob 模式，如 '*.csv'" },
                "path": { "type": "string", "description": "搜索的子目录路径（绝对或相对授权工作目录），默认 '.'", "default": "." },
                "max_results": { "type": "integer", "description": "最多返回结果数，默认 100", "default": 100 }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new("Write", "在授权工作目录中创建或完整覆盖写入文本文件")
            .with_kind(ToolKind::Primitive)
            .with_capability_scope(["workspace:write"]),
        json!({
            "type": "object",
            "required": ["file_path", "content"],
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "目标文件路径（绝对或相对授权工作目录）"
                },
                "content": {
                    "type": "string",
                    "description": "要写入的文件完整内容（UTF-8 文本）。必须在同一次调用中提供全部内容，不得分步调用或省略任何部分。"
                }
            },
            "description": "将文本内容写入工作目录中的文件。\n\n使用规则：\n- 如果目标文件已存在，必须先使用 Read 工具读取其内容，否则本工具将拒绝执行。\n- 修改已有文件时，优先使用 Edit 工具（仅传输差异部分）。仅在新建文件或需要完整重写时使用本工具。\n- content 参数必须在同一次调用中包含文件的全部最终内容，不得分批或分步写入。\n- 本工具会创建不存在的父目录。"
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "Edit",
            "对授权工作目录中的文件执行精确的 old_string → new_string 替换（优先于 Write 用于修改现有文件）",
        )
        .with_kind(ToolKind::Primitive)
        .with_capability_scope(["workspace:read", "workspace:write"]),
        json!({
            "type": "object",
            "required": ["file_path", "old_string", "new_string"],
            "properties": {
                "file_path": { "type": "string", "description": "文件路径（绝对或相对授权工作目录）" },
                "old_string": {
                    "type": "string",
                    "description": "要替换的原始字符串。默认必须在文件中唯一存在，否则工具报错；若需替换全部出现，请设置 replace_all=true。若为空字符串，则视为向空文件写入内容（文件必须为空或不存在）。"
                },
                "new_string": { "type": "string", "description": "替换后的新字符串" },
                "replace_all": {
                    "type": "boolean",
                    "description": "true 时替换文件中所有出现的 old_string；false（默认）时要求 old_string 唯一存在",
                    "default": false
                }
            },
            "description": "对文件执行精确字符串替换。\n\n使用规则：\n- 编辑前必须至少使用一次 Read 读取目标文件，否则本工具将报错。\n- 修改现有文件时始终优先使用本工具，而非 Write（本工具只传输差异，更安全高效）。\n- 默认要求 old_string 在文件中唯一；不唯一时请扩大 old_string 的上下文，或传 replace_all=true 替换全部。\n- old_string 和 new_string 必须保持原始缩进（空格/Tab），不得修改缩进格式。"
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "Bash",
            "在授权工作目录中执行 shell 命令。默认 timeout 120000ms；当前前台路径在 timeout/cancel 时终止进程并返回错误。\
            \n\n安全约束：仅对明显危险 pattern（`rm -rf /`、向 /etc/ 写入等）做 hard deny。\
            \n\nstdout + stderr 合并返回；非零 exit code 默认按错误处理，grep/rg/find/diff/test 等遵循 claude-code-best 的语义豁免。",
        )
        .with_kind(ToolKind::Primitive)
        .with_destructive(true)
        .with_default_timeout_secs(120)
        .with_capability_scope(["workspace:write"]),
        json!({
            "type": "object",
            "required": ["command"],
            "properties": {
                "command": { "type": "string", "description": "要执行的 shell 命令" },
                "timeout": {
                    "type": "integer",
                    "description": "超时毫秒数，默认 120000（120 秒），最大 600000（10 分钟）",
                    "default": 120000
                }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "PowerShell",
            "在授权工作目录中执行 PowerShell 命令（Windows 平台专用）。\
            优先使用 pwsh.exe（PowerShell 7+，支持 `&&` `||`），否则回退 powershell.exe（5.1，**不支持 `&&`/`||`**，请用 `;` 分隔或显式判断 `$LASTEXITCODE`）。\
            \n\n用法说明：\
            \n- 文件操作：`Get-ChildItem`、`Get-Content`、`Remove-Item -Recurse -Force`\
            \n- 文本搜索：`Select-String -Pattern 'foo' -Path *.txt`（grep 等价）\
            \n- 调用 .exe：直接写程序名即可，如 `python script.py`、`node app.js`\
            \n- **不要**使用 Unix 专属命令（grep/find/rm/cat/ls -la 等不存在或行为不同）\
            \n\n默认 timeout 120000ms；timeout/cancel 时终止进程并返回错误。\
            \n\n安全约束：拒绝 `Remove-Item C:\\Windows`、`Format-Volume`、`Stop-Computer`、`iwr ... | iex` 等危险模式。\
            \n\nstdout + stderr 合并返回；非零 exit code 默认按错误处理。",
        )
        .with_kind(ToolKind::Primitive)
        .with_destructive(true)
        .with_default_timeout_secs(120)
        .with_capability_scope(["workspace:write"]),
        json!({
            "type": "object",
            "required": ["command"],
            "properties": {
                "command": { "type": "string", "description": "要执行的 PowerShell 命令" },
                "timeout": {
                    "type": "integer",
                    "description": "超时毫秒数，默认 120000（120 秒），最大 600000（10 分钟）",
                    "default": 120000
                }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "Grep",
            "在授权工作目录中搜索文件内容。当前 Phase 1 对标 claude-code-best 的 GrepTool 核心模式：\
            \n- `output_mode=files_with_matches`：返回命中文件路径\
            \n- `output_mode=content`：返回 `path:line:content` 文本\
            \n- `output_mode=count`：返回 `path:count` 文本\
            \n\n当前不包含 `type/head_limit/offset/multiline/context/-i` 等扩展参数。",
        )
        .with_kind(ToolKind::Primitive)
        .with_read_only(true)
        .with_capability_scope(["workspace:read"]),
        json!({
            "type": "object",
            "required": ["pattern"],
            "properties": {
                "pattern": { "type": "string", "description": "要搜索的正则表达式模式" },
                "path": {
                    "type": "string",
                    "description": "相对于 workspace root 的搜索起点（文件或目录），默认 '.'",
                    "default": "."
                },
                "glob": {
                    "type": "string",
                    "description": "可选文件名 glob，仅支持简单 * 通配符，如 '*.rs'"
                },
                "output_mode": {
                    "type": "string",
                    "enum": ["content", "files_with_matches", "count"],
                    "description": "结果模式，默认 files_with_matches",
                    "default": "files_with_matches"
                }
            }
        }),
    ));

    // ── Primitive: network ────────────────────────────────────────
    c.insert(CatalogEntry::new(
        ToolDefinition::new("WebSearch", "搜索互联网获取最新信息")
            .with_kind(ToolKind::Primitive)
            .with_capability_scope(["network"]),
        json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": { "type": "string", "description": "搜索词" },
                "max_results": { "type": "integer", "description": "最多返回结果数，默认 5", "default": 5 }
            }
        }),
    ));


    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "Agent",
            "【Composite 工具】启动一个子 Agent 执行聚焦任务。\
            \n\n适用场景：任务需要干净上下文、专属 Agent 类型（如 'explore'、'general-purpose'）或不同模型。\
            \n\n同步路径（run_in_background=false 或省略）：阻塞等待子 Agent 完成并返回最终输出文本。\
            \n\n异步路径（run_in_background=true）：立即返回 agent_id；子 Agent 在后台运行；用 TaskOutput(task_id=agent_id, offset=N) 增量读取 transcript；子 Agent 完成时父的下一轮会收到 <task-notification> XML。",
        )
        .with_kind(ToolKind::Composite)
        .with_capability_scope(["workspace:write"]),
        json!({
            "type": "object",
            "required": ["subagent_type", "prompt", "description"],
            "properties": {
                "subagent_type": {
                    "type": "string",
                    "description": "Agent 类型名称，来自注册表（如 'general-purpose'、'explore'）。"
                },
                "prompt": {
                    "type": "string",
                    "description": "子 Agent 应执行的完整任务指令。"
                },
                "description": {
                    "type": "string",
                    "description": "3-5 词任务描述，用于日志和 UI 展示。"
                },
                "model": {
                    "type": "string",
                    "description": "为该子 Agent 调用覆盖模型（如 'haiku'）。省略则继承父 Agent 的模型。"
                },
                "run_in_background": {
                    "type": "boolean",
                    "description": "若为 true，异步运行并立即返回 agent_id；后续用 TaskOutput 增量读 transcript，完成时父的下一轮收到 <task-notification>。",
                    "default": false
                },
                "name": {
                    "type": "string",
                    "description": "可选的实例名，用于 SendMessage 路由到异步子 Agent（仅异步模式使用）。"
                }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "TaskOutput",
            "【Support 工具】读取异步子 Agent 的 transcript 增量。\
            \n\n用法：Agent({run_in_background: true, name: \"w1\"}) 立即返回 agent_id。\
            子 Agent 完成时通过 <task-notification> XML 通知（含 <output-file> 路径）。\
            期间或之后用 TaskOutput(task_id=agent_id, offset=N) 读取产出。\
            \n\n返回 {lines: [string], new_offset: number}。下次调用传 offset=new_offset 拉取增量。",
        )
        .with_kind(ToolKind::Support)
        .with_read_only(true),
        json!({
            "type": "object",
            "required": ["task_id"],
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "异步 Agent 的 ID（Agent 工具 run_in_background=true 时返回的 agent_id）"
                },
                "offset": {
                    "type": "integer",
                    "description": "起始行偏移（默认 0）",
                    "default": 0,
                    "minimum": 0
                }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "Skill",
            "加载一个专项技能的详细指令到当前对话。无副作用：不改变系统提示、不限制工具、不持久化。",
        )
        .with_kind(ToolKind::Support)
        .with_read_only(true)
        .with_max_result_size_chars(16_000)
        .with_preserve_tool_use_results(true),
        json!({
            "type": "object",
            "required": ["skill_id"],
            "properties": {
                "skill_id": {
                    "type": "string",
                    "description": "技能 ID，必须来自动态上下文中的可用专项技能目录"
                }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "AskUserQuestion",
            "向用户提出结构化多选问题，等待用户回答后继续。\
            \n\n用途：收集用户偏好、澄清歧义、让用户在多个方案中选择。\
            \n\n每次调用支持 1-4 个问题，每个问题 2-4 个选项，用户始终可以选择 Other 输入自定义回答。",
        )
        .with_kind(ToolKind::Support)
        .with_read_only(true),
        json!({
            "type": "object",
            "required": ["questions"],
            "properties": {
                "questions": {
                    "type": "array",
                    "description": "要向用户提出的问题列表（1-4 个）",
                    "minItems": 1,
                    "maxItems": 4,
                    "items": {
                        "type": "object",
                        "required": ["question", "header", "options"],
                        "properties": {
                            "question": { "type": "string" },
                            "header": { "type": "string" },
                            "options": {
                                "type": "array",
                                "minItems": 2,
                                "maxItems": 4,
                                "items": {
                                    "type": "object",
                                    "required": ["label", "description"],
                                    "properties": {
                                        "label": { "type": "string" },
                                        "description": { "type": "string" },
                                        "preview": { "type": "string" }
                                    }
                                }
                            },
                            "multiSelect": { "type": "boolean", "default": false }
                        }
                    }
                },
                "answers": { "type": "object", "description": "用户回答（由系统填入，模型勿填）" },
                "metadata": { "type": "object" }
            }
        }),
    ));

    // ── Support: memory tools ─────────────────────────────────────

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "TaskCreate",
            "创建一条持久化任务，用于当前 session/agent 工作清单。",
        )
        .with_kind(ToolKind::Support),
        json!({
            "type": "object",
            "required": ["subject", "description"],
            "properties": {
                "subject": { "type": "string", "description": "任务短标题" },
                "description": { "type": "string", "description": "任务详细说明" },
                "activeForm": { "type": "string", "description": "进行中展示文案，如 Running tests" },
                "metadata": { "type": "object", "description": "可选元数据" }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "TaskUpdate",
            "更新、删除或设置任务依赖、owner、status、metadata。",
        )
        .with_kind(ToolKind::Support),
        json!({
            "type": "object",
            "required": ["taskId"],
            "properties": {
                "taskId": { "type": "string", "description": "任务 ID" },
                "subject": { "type": "string", "description": "新的任务标题" },
                "description": { "type": "string", "description": "新的任务描述" },
                "activeForm": { "type": "string", "description": "进行中展示文案" },
                "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "deleted"] },
                "owner": { "type": "string", "description": "任务 owner agent/name" },
                "addBlocks": { "type": "array", "items": { "type": "string" } },
                "addBlockedBy": { "type": "array", "items": { "type": "string" } },
                "metadata": { "type": "object", "description": "metadata merge；value=null 表示删除 key" }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new("TaskList", "列出当前 task list 的所有任务及阻塞状态。")
            .with_kind(ToolKind::Support)
            .with_read_only(true),
        json!({
            "type": "object",
            "properties": {},
            "required": []
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "TaskGet",
            "按 taskId 获取单条任务的完整记录（含 metadata / blocks / blockedBy）。",
        )
        .with_kind(ToolKind::Support)
        .with_read_only(true),
        json!({
            "type": "object",
            "required": ["taskId"],
            "properties": {
                "taskId": { "type": "string" }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "TaskStop",
            "终止一个正在后台运行的 Agent 任务（按 task_id，即 Agent(run_in_background=true) 返回的 agent_id 同值）。",
        )
        .with_kind(ToolKind::Support),
        json!({
            "type": "object",
            "required": ["task_id"],
            "properties": {
                "task_id": { "type": "string", "description": "后台 Agent 任务 ID，与 agent_id 值相同" }
            }
        }),
    ));

    // ── Power: skill-smith (小程) tools ────────────────────────────────
    for (def, schema) in crate::runtime::tools::builtin::skill_smith::catalog_entries() {
        c.insert(CatalogEntry::new(def, schema));
    }

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "WriteMemory",
            "保存一条项目记忆到本地记忆库。记忆按 workspace 分桶存储，跨对话持久化。\n\n类型说明：\n- user_preference：用户偏好\n- project_constraint：项目约束\n- reference_info：外部系统指针\n- feedback：AI 行为纠正或确认",
        )
        .with_kind(ToolKind::Support),
        json!({
            "type": "object",
            "required": ["name", "memory_type", "description", "content"],
            "properties": {
                "name": {
                    "type": "string",
                    "description": "记忆条目名称，简短唯一，用于索引"
                },
                "memory_type": {
                    "type": "string",
                    "enum": ["user_preference", "project_constraint", "reference_info", "feedback"],
                    "description": "记忆类型"
                },
                "description": {
                    "type": "string",
                    "description": "一句话描述，用于未来相关性判断"
                },
                "content": {
                    "type": "string",
                    "description": "记忆正文；feedback 类型建议包含规则本体、Why、How to apply"
                }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "SearchMemory",
            "在本地记忆库中按关键词搜索相关记忆条目，返回最多 5 条最相关结果。",
        )
        .with_kind(ToolKind::Support)
        .with_read_only(true),
        json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {
                    "type": "string",
                    "description": "搜索关键词或问题描述"
                }
            }
        }),
    ));

    c
}

/// daily 模式允许 LLM 直接调用的工具集（Primitive + 必要 Support 工具）。
///
/// 对齐原子工具模型；register_runtime 注册的工具默认走 ToolDispatcher。
/// `Skill` 是例外：它需要 request-scoped SkillRegistry，但必须在 daily 模式可见。
pub const DAILY_ALLOWED_TOOLS: &[&str] = &[
    // 以下 10 个工具均在 register_builtin_tools() 中 register_runtime 注册，走 ToolDispatcher
    // Shell：每平台只注册其中一个（Unix=bash, Windows=powershell），过滤层会自动隐藏不可达的那个
    "Bash",
    "PowerShell",
    "Read",
    "Write",
    "Edit",
    "Glob",
    "Grep",
    "WriteMemory",
    "SearchMemory",
    "Agent",
    "TaskOutput",
    "Skill",
    "AskUserQuestion",
    "TaskCreate",
    "TaskUpdate",
    "TaskList",
    "TaskGet",
    "TaskStop",
    "skill_create_draft",
    "skill_write_md",
    "skill_add_file",
    "skill_validate",
    "skill_dry_run",
    "skill_install",
];

/// 全局默认 catalog（延迟初始化）。
pub static TOOL_CATALOG: LazyLock<DynamicToolCatalog> =
    LazyLock::new(DynamicToolCatalog::new_with_defaults);
