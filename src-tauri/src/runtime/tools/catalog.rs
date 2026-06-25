//! ToolCatalog — 工具元数据的单一真相源。
//!
//! 所有工具（primitive/power/composite/support）都在此注册。
//! `llm/tools.rs` 中的旧 schema 定义降级为兼容层，不再新增。
//! `plugin/registry.rs` 的运行时注册以本 catalog 为权威来源。

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

use serde_json::{Value, json};

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
        ToolDefinition::new(
            "Read",
            "读取授权工作目录中的文本文件内容。不要用本工具检查二进制、媒体、压缩包、模型、数据库或其他结构化二进制数据的原始内容；PNG/JPG/PDF/音视频/压缩包/STL/Parquet/SQLite 等不会返回可用于判断的完整正文。遇到这类文件时，应改用元数据、OCR、截图、专用解析器，或直接写并运行解析脚本生成用户要求的目标文件；如果用户已经给出二进制结构、字段、schema 或输出格式，不要先 Read 二进制文件探测正文，应让脚本读取该文件并写出目标产物。大文件可能返回 truncated=true；若被截断部分影响结论，应使用 offset/limit、搜索或脚本切片继续读取，不要把预览当完整证据，也不要反复整读大型 JSON/CSV/日志。",
        )
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
        ToolDefinition::new("Glob", "在授权工作目录中按文件名递归搜索匹配 glob 模式的文件。适合低成本发现输入文件、输出目录、AGENTS.md 和本地技能说明。非平凡文件任务的第一轮发现不要只搜用户点名的输入文件，还应搜索 `SKILL.md`（例如 `Glob(pattern=\"SKILL.md\")`），再从返回路径中识别当前目录、`.agents/skills/*/SKILL.md` 或 `skills/*/SKILL.md`；发现明显相关项后用 Read 读取。本地 SKILL.md 不是动态 Skill 工具的 skill_id，不要用 Skill(find-skills) 代替本工具查本地文件。")
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
            "description": "将文本内容写入工作目录中的文件。\n\n使用规则：\n- 如果目标文件已存在，必须先使用 Read 工具读取其内容，否则本工具将拒绝执行。\n- 修改已有文件时，优先使用 Edit 工具（仅传输差异部分）。仅在新建文件或需要完整重写时使用本工具。\n- content 参数必须在同一次调用中包含文件的全部内容，不得分批或分步写入。\n- 本工具会创建不存在的父目录。\n\n失败恢复：\n- 如果本工具因“文件已存在但未读取”被拒绝，下一步先 Read 目标文件，再使用 Edit 或完整重写；不要改用 Bash/PowerShell 直接截断覆盖来绕过读取约束。\n- 如果父目录、权限或路径错误，先确认用户指定路径和授权工作目录，再创建缺失目录或写入结构化阻塞原因；不要把文件写到临时目录后宣称完成。\n\n交付规则：\n- 用户指定了明确文件名、输出路径或 schema 时，本工具写出的内容应当尽量就是可交付版本，而不是“稍后补全”的占位稿。\n- JSON/CSV/配置/API payload 等结构化产物必须符合用户给出的字段和格式；除非关键输入、权限或工具真实阻塞，否则不要写 null、TODO、status: computing、placeholder、虚构数值或多余说明字段。\n- 若当前已具备计算、解析或转换所需输入，优先用 Bash/PowerShell/项目脚本直接生成真实结果文件，再用 Read/解析命令验证；不要先写一个会被评分器当成最终结果的临时 JSON。写脚本、生成脚本或保存 helper 只是中间步骤；除非用户明确要求脚本本身，否则下一步必须运行它生成用户命名的最终文件并验证该文件。\n- 调度、排产、资源分配、权限分配或任何带硬约束的最终文件，写出的最终字段本身必须满足约束；`attendees`/`ATTENDEE`/`assignees`/`resources`/`equipment` 中出现的 optional、candidate、backup 也按已安排/已分配处理。ICS 中 `ATTENDEE;...` 带参数也仍是 `ATTENDEE`，例如 `ATTENDEE;ROLE=OPT-PARTICIPANT:mailto:x` 必须按已安排参会人校验。不要默认把 required+optional 或候选/备用全集写入最终字段；先按同一套硬约束过滤可选项。写入后下一步验证不能只检查文件存在、事件数量或 JSON 可解析，必须对最终文件做字段级断言；断言必须保留并比较完整邮箱/账号/资源 ID，不得把 `carol@company.com` 截成 `carol@` 或只比显示名；断言失败要立即 Edit/重写目标文件。\n- 用户给出 exactly/following structure/schema/template、固定章节、固定字段或固定文件集合时，最终文件不要添加额外顶层章节、调试统计、过程表格或多余字段；验证要比较章节名、字段名和文件列表，失败时立即 Edit/重写。\n- 确实阻塞时，可以写结构化阻塞记录，但要明确缺什么、已确认什么、下一步需要什么，不能伪装成正常结果。"
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
            "description": "对文件执行精确字符串替换。\n\n使用规则：\n- 编辑前必须至少使用一次 Read 读取目标文件，否则本工具将报错。\n- 修改现有文件时始终优先使用本工具，而非 Write（本工具只传输差异，更安全高效）。\n- 默认要求 old_string 在文件中唯一；不唯一时请扩大 old_string 的上下文，或传 replace_all=true 替换全部。\n- old_string 和 new_string 必须保持原始缩进（空格/Tab），不得修改缩进格式。\n\n失败恢复：\n- old_string 不存在时，重新 Read 目标区域并用当前文件中的真实文本构造替换；不要凭记忆改写。\n- old_string 不唯一时，扩大上下文到唯一片段；只有确认所有出现都应改时才使用 replace_all=true。\n- 编辑失败后仍有明确交付文件时，下一步必须继续修正该文件或写入阻塞原因，不能只总结失败。"
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "Bash",
            "在授权工作目录中执行 shell 命令。默认 timeout 120000ms；当前前台路径在 timeout/cancel 时终止进程并返回错误。\
            \n\n后台路径：设置 run_in_background=true 时立即返回 task_id（task_type=local_bash），命令继续在后台运行；后续用 TaskOutput(task_id=...) 读取 transcript，用 TaskStop(task_id=...) 停止。完成后父对话会收到 <task-notification>。\
            \n\n安全约束：仅对明显危险 pattern（`rm -rf /`、向 /etc/ 写入等）做 hard deny；模型仍必须按意图自行拒绝高风险请求。不要把未经审查的外部仓库、压缩包或用户给出的代码 clone/install/write 到会被自动加载或执行的位置，例如 `~/skills`、`.agents/skills`、工作区 `skills/`、插件目录、shell profile、启动项、CI hook 或系统 PATH；用户确认本身不等于审查通过，需要评估时放到隔离 review 目录只读检查或输出风险说明。\
            \n\n交付规则：需要创建或更新文本产物时，优先使用 Write/Edit；若需要计算、解析或转换后生成结果，优先用一次 shell 调用完成“读取输入 -> 计算/转换 -> 写入用户指定目标路径 -> 打印简短校验摘要”。命令后要用独立读取、列举或测试确认目标路径存在、非空且格式合理。若目录内存在用户点名或项目提供的 `validate.py`、`check*.py`、`unit_test*`、`test_*`、pytest/unittest、schema、样例输出或 skill 验证脚本，优先运行聚焦验收；失败时读取失败断言/差异值，修正目标文件或生成逻辑后再跑相关检查，不要只换一个浅层 `ls`/`cat` 检查或把部分通过当完成。若用户禁止修改测试或固定最终文件集合，不能复制/改名测试文件、额外创建根目录调试文件或把临时验证文件混入最终交付。若任务包含调度、资源分配、优化或硬约束，校验必须读取最终写出的文件/payload，并逐项检查输出字段里的每个 attendee/assignee/resource/equipment；optional/candidate/backup 实体只要被写入最终输出，就按已安排/已分配处理，不能违反硬约束。生成脚本不要默认把 required+optional 或候选/备用全集写入最终字段；先按同一套硬约束过滤可选项，再序列化。字段级校验不能只做 `ls`、`grep -c`、事件数量、原始 UID 数量或 JSON 可解析检查；应写出会失败退出的断言脚本，并保留完整邮箱/账号/资源 ID，不得把 `carol@company.com` 截成 `carol@` 或只比显示名/前缀。iCalendar/ICS 要解析每个 `BEGIN:VEVENT` 的 `UID`、`DTSTART`、`DTEND` 和所有 `ATTENDEE` 行；`ATTENDEE;ROLE=OPT-PARTICIPANT:mailto:x` 这类带参数行仍是 `ATTENDEE`，必须和 unavailability/容量/冲突规则比对；JSON/CSV 要解析 schema 和关键字段；Markdown 精确结构要检查必需标题且避免额外章节。若用户给出 exactly/following structure/schema/template、固定章节或固定字段，断言脚本要比较章节名/字段名，不能添加调试统计或额外顶层章节。断言失败时先修目标文件，再总结。\
            明确命名的 JSON/CSV/配置/API payload 不要先写 null、status: computing、TODO 或 placeholder 占位；如果输入和规则已经足够，直接生成真实字段值。若命令创建临时脚本，必须在同一次调用或下一步立即执行它并写回目标文件，不要只留下 /tmp 脚本或 stdout。脚本退出成功但用户命名的 PNG/PDF/JSON/CSV/MD/HTML 等最终产物不存在、为空或不可解析时，仍按交付失败处理，下一步先修脚本或输出路径并重跑。\
            \n\n失败恢复：先按错误类型决定下一步。命令不存在、依赖缺失、路径不可达或超时时，不要反复原样重试；改用已安装工具、小脚本、项目校验命令或把真实阻塞原因写入要求的产物。shell 语法错误时修正为当前 shell 语法；路径错误时先列举/定位授权工作目录和目标父目录；输出过长或被截断时把完整结果写入文件，再分段读取。网络/5xx/429/超时应减少并发、延长合理 timeout、退避重试一次或写入阻塞，不要把服务波动当成业务结论。权限或安全拒绝表示边界命中，不要换写法绕过，应说明风险、请求授权或提供安全替代。解析 STL/Parquet/SQLite/压缩包等结构化二进制数据时，不要优先用 `xxd`、`hexdump`、`od` 或 `file` 探正文；优先用 Python、Node、项目 helper 或专用解析器直接读取文件并写出目标产物。若生成 PNG/图表等二进制产物时缺少 matplotlib/Pillow 等包，不要卡在 pip install；优先用已安装库，或用 Python 标准库写入可检查的简版 PNG/SVG 替代实现，并验证目标文件存在非空。\
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
                },
                "description": {
                    "type": "string",
                    "description": "命令用途的简短描述，用于后台任务列表和完成通知"
                },
                "run_in_background": {
                    "type": "boolean",
                    "description": "true 时后台运行并立即返回 task_id；后续用 TaskOutput 读取，用 TaskStop 停止",
                    "default": false
                }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "PowerShell",
            "在授权工作目录中执行 PowerShell 命令（Windows 平台专用）。\
            优先使用 pwsh.exe（PowerShell 7+，支持 `&&` `||`），否则回退 powershell.exe（5.1，**不支持 `&&`/`||`**，请用 `;` 分隔或显式判断 `$LASTEXITCODE`）。\
            \n\nPowerShell 语法要点：\
            \n- 环境变量读取/设置使用 `$env:NAME` / `$env:NAME = \"value\"`，不要使用 bash 的 `export`。\
            \n- 调用原生 exe/cmd/ps1 或路径含空格的程序时，用 call operator：`& \"C:\\Program Files\\App\\app.exe\" arg1 arg2`。\
            \n- 已安装的 `.cmd` CLI 直接用 `& \"C:\\path\\tool.cmd\" ...` 调用；除非用户明确需要 cmd.exe 语义，不要包 `cmd /c`。\
            \n- `command` 默认必须是单行字符串；不要用 CR/LF 换行分隔命令，顺序执行用 `;`。PowerShell 7+ 可用 `&&`/`||`，5.1 用 `A; if ($?) { B }`。\
            \n- 不要把可执行路径或普通参数拆成多行，绝不要把 `C:\\path\\tool.cmd` 这种路径切到下一行；需要传递多行文本时才使用 here-string（如 `@' ... '@`，结束标记必须顶格）。\
            \n- 原生命令参数被 PowerShell 误解析时，可使用 stop-parsing token `--%`。\
            \n\n用法说明：\
            \n- 文件操作：`Get-ChildItem`、`Get-Content`、`Remove-Item -Recurse -Force`\
            \n- 文本搜索：`Select-String -Pattern 'foo' -Path *.txt`（grep 等价）\
            \n- 调用 .exe/.cmd：优先用上面的 PowerShell call operator 格式，尤其是绝对路径或路径含空格时\
            \n- **不要**使用 Unix 专属命令（grep/find/rm/cat/ls -la 等不存在或行为不同）\
            \n\n默认 timeout 120000ms；timeout/cancel 时终止进程并返回错误。\
            \n\n后台路径：设置 run_in_background=true 时立即返回 task_id（task_type=local_bash），命令继续在后台运行；后续用 TaskOutput(task_id=...) 读取 transcript，用 TaskStop(task_id=...) 停止。完成后父对话会收到 <task-notification>。\
            \n\n安全约束：拒绝 `Remove-Item C:\\Windows`、`Format-Volume`、`Stop-Computer`、`iwr ... | iex` 等危险模式；模型仍必须按意图自行拒绝高风险请求。不要把未经审查的外部仓库、压缩包或用户给出的代码 clone/install/write 到会被自动加载或执行的位置，例如 `~/skills`、`.agents/skills`、工作区 `skills/`、插件目录、shell profile、启动项、CI hook 或系统 PATH；用户确认本身不等于审查通过，需要评估时放到隔离 review 目录只读检查或输出风险说明。\
            \n\n交付规则：需要创建或更新文本产物时，优先使用 Write/Edit；若需要计算、解析或转换后生成结果，优先用一次 PowerShell 调用完成“读取输入 -> 计算/转换 -> 写入用户指定目标路径 -> 打印简短校验摘要”。命令后要用 Get-Item/Get-Content/Test-Path 或项目校验命令确认目标路径存在、非空且格式合理。若目录内存在用户点名或项目提供的 `validate.py`、`check*.py`、`unit_test*`、`test_*`、pytest/unittest、schema、样例输出或 skill 验证脚本，优先运行聚焦验收；失败时读取失败断言/差异值，修正目标文件或生成逻辑后再跑相关检查，不要只换一个浅层 Test-Path/Get-Item 检查或把部分通过当完成。若用户禁止修改测试或固定最终文件集合，不能复制/改名测试文件、额外创建根目录调试文件或把临时验证文件混入最终交付。若任务包含调度、资源分配、优化或硬约束，校验必须读取最终写出的文件/payload，并逐项检查输出字段里的每个 attendee/assignee/resource/equipment；optional/candidate/backup 实体只要被写入最终输出，就按已安排/已分配处理，不能违反硬约束。生成脚本不要默认把 required+optional 或候选/备用全集写入最终字段；先按同一套硬约束过滤可选项，再序列化。字段级校验不能只做 Test-Path、目录列举、事件数量、原始 UID 数量或 JSON 可解析检查；应写出会失败退出的断言脚本，并保留完整邮箱/账号/资源 ID，不得把 `carol@company.com` 截成 `carol@` 或只比显示名/前缀。iCalendar/ICS 要解析每个 `BEGIN:VEVENT` 的 `UID`、`DTSTART`、`DTEND` 和所有 `ATTENDEE` 行；`ATTENDEE;ROLE=OPT-PARTICIPANT:mailto:x` 这类带参数行仍是 `ATTENDEE`，必须和 unavailability/容量/冲突规则比对；JSON/CSV 要解析 schema 和关键字段；Markdown 精确结构要检查必需标题且避免额外章节。若用户给出 exactly/following structure/schema/template、固定章节或固定字段，断言脚本要比较章节名/字段名，不能添加调试统计或额外顶层章节。断言失败时先修目标文件，再总结。\
            明确命名的 JSON/CSV/配置/API payload 不要先写 null、status: computing、TODO 或 placeholder 占位；如果输入和规则已经足够，直接生成真实字段值。若命令创建临时脚本，必须在同一次调用或下一步立即执行它并写回目标文件，不要只留下临时脚本或 stdout。脚本退出成功但用户命名的 PNG/PDF/JSON/CSV/MD/HTML 等最终产物不存在、为空或不可解析时，仍按交付失败处理，下一步先修脚本或输出路径并重跑。\
            \n\n失败恢复：先按错误类型决定下一步。命令不存在、模块缺失、路径不可达或超时时，不要反复原样重试；改用已安装工具、小脚本、项目校验命令或把真实阻塞原因写入要求的产物。PowerShell 5.1/7 语法差异导致失败时，改用当前版本支持的分隔、环境变量和 call operator；路径错误时先用 Test-Path/Get-ChildItem 定位授权工作目录和目标父目录；输出过长或被截断时写入文件再分段读取。网络/5xx/429/超时应减少并发、延长合理 timeout、退避重试一次或写入阻塞，不要把服务波动当成业务结论。权限或安全拒绝表示边界命中，不要换写法绕过，应说明风险、请求授权或提供安全替代。若生成 PNG/图表等二进制产物时缺少 matplotlib/Pillow 等包，不要卡在安装；优先用已安装库，或用 Python 标准库写入可检查的简版 PNG/SVG 替代实现，并验证目标文件存在非空。\
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
                "command": { "type": "string", "description": "要执行的 PowerShell 命令；默认必须是单行字符串，顺序执行用 ;，不要把可执行路径或普通参数拆成多行" },
                "timeout": {
                    "type": "integer",
                    "description": "超时毫秒数，默认 120000（120 秒），最大 600000（10 分钟）",
                    "default": 120000
                },
                "description": {
                    "type": "string",
                    "description": "命令用途的简短描述，用于后台任务列表和完成通知"
                },
                "run_in_background": {
                    "type": "boolean",
                    "description": "true 时后台运行并立即返回 task_id；后续用 TaskOutput 读取，用 TaskStop 停止",
                    "default": false
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
            "ImageTask",
            "创建或编辑图片的产品级工具。用于文生图、基于参考图改图、生成变体；输入使用 AIjia 图片任务协议，不暴露 LLM 或上游模型供应商字段。\
            \n\n使用规则：\
            \n- 文生图使用 action=image.create，可不传 input_images。\
            \n- 图生图、参考图编辑、风格迁移或变体使用 action=image.edit 或 image.variation，并传 input_images。\
            \n- input_images 可使用用户附件中的 filePath/fileId；不要传 provider 字段如 image、response_format、model。\
            \n- 本工具不是图片查看、OCR、图表解析或视觉问答工具；它返回生成/编辑后的图片文件，不代表已经理解了输入图内容。\
            \n- 如果只是需要从图片/视频/页面生成报告、HTML、SVG、代码或数据文件，优先使用查看、OCR、截图、元数据或专用解析工具；没有这些能力但用户已给出明确规格时，直接基于规格创建目标产物并标明未验证细节。\
            \n- 工具会把生成图片保存为当前会话的 generated file，并返回 fileId。",
        )
        .with_kind(ToolKind::Power)
        .with_destructive(true)
        .with_default_timeout_secs(180)
        .with_capability_scope(["network", "workspace:read", "workspace:write"]),
        json!({
            "type": "object",
            "required": ["action", "instruction"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["image.create", "image.edit", "image.variation"],
                    "description": "图片任务类型：文生图、基于输入图编辑、或生成输入图变体"
                },
                "instruction": {
                    "type": "string",
                    "description": "面向产品能力的图片生成/编辑意图。描述要保留、改变、风格、构图、颜色、文字等要求"
                },
                "input_images": {
                    "type": "array",
                    "description": "参考图片、源图或蒙版。image.edit/image.variation 必填至少一张",
                    "items": {
                        "type": "object",
                        "properties": {
                            "file_path": {
                                "type": "string",
                                "description": "图片文件路径。可使用用户附件 files[].filePath，或相对当前授权工作目录的路径"
                            },
                            "file_id": {
                                "type": "string",
                                "description": "图片文件 ID。可使用用户附件 files[].id 或已生成图片的 fileId"
                            },
                            "role": {
                                "type": "string",
                                "enum": ["source", "reference", "style_reference", "composition_reference", "mask"],
                                "description": "图片用途，默认 source"
                            },
                            "mime_type": {
                                "type": "string",
                                "description": "图片 MIME，如 image/png、image/jpeg、image/webp。通常可由扩展名推断"
                            },
                            "weight": {
                                "type": "number",
                                "description": "参考权重，按需传递"
                            }
                        },
                        "anyOf": [
                            { "required": ["file_path"] },
                            { "required": ["file_id"] }
                        ]
                    },
                    "default": []
                },
                "output": {
                    "type": "object",
                    "description": "输出要求",
                    "properties": {
                        "count": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 10,
                            "description": "生成图片数量，默认 1"
                        },
                        "aspect_ratio": {
                            "type": "string",
                            "enum": ["1:1", "16:9", "9:16", "4:3", "3:4"],
                            "description": "输出比例"
                        },
                        "width": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "输出宽度。通常优先使用 aspect_ratio"
                        },
                        "height": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "输出高度。通常优先使用 aspect_ratio"
                        },
                        "format": {
                            "type": "string",
                            "enum": ["png", "jpeg", "jpg", "webp"],
                            "description": "输出格式，默认 png"
                        },
                        "quality": {
                            "type": "string",
                            "enum": ["standard", "high"],
                            "description": "输出质量"
                        }
                    }
                }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "Agent",
            "【Composite 工具】启动一个子 Agent 执行聚焦任务。\
            \n\n适用场景：任务需要干净上下文、专属 Agent 类型或不同模型。`subagent_type` 取值范围在每轮 turn 的工具描述动态列表中给出，包含 builtin 类型、用户自定义 agent、以及当前用户已雇佣的数字员工 ID（`emp-...`）。\
            \n\n不适用场景：不要为了“再确认一下”、未知能力探测、普通文件读取、图片查看、OCR、网页浏览或同一任务的重复尝试而启动子 Agent，除非该 Agent 的描述明确拥有父 Agent 没有的能力。\
            \n\n交付责任：父 Agent 仍负责最终交付检查。调用 Agent 时要在 prompt 中写清输入路径、输出文件、完成标准和需要回传的证据；子 Agent 返回后，父 Agent 必须验证命名文件、配置、脚本或报告已经真实落地，不能把子 Agent 的文字总结直接当成交付。\
            \n\n默认路径（run_in_background=false 或省略）：子 Agent 先以前台方式运行；如果在前台阻塞预算内完成，直接返回最终输出文本；如果超过预算，系统会自动返回 `task_id`（`task_type=local_agent`）并让同一个子 Agent 继续在后台执行。\
            \n\n异步路径（run_in_background=true）：立即返回 `agent_id/task_id`（`task_type=local_agent`）；子 Agent 从一开始就在后台运行。后台任务都可用 TaskOutput(task_id=..., task_type=\"local_agent\", offset=N) 增量读取 transcript；子 Agent 完成时父的下一轮会收到 <task-notification> XML。\
            \n\nTeammate 派活路径（显式传 team_name + name）：加入当前 Session 的 Team 作为 Teammate 运行。`subagent_type` 可以是 builtin/通用 Agent，也可以是用户明确要求或确需其专属能力的数字员工 `emp-...`。省略 `team_name` 时即使当前已有 active team，也按普通独立子 Agent 运行。",
        )
        .with_kind(ToolKind::Composite)
        .with_capability_scope(["workspace:write"]),
        json!({
            "type": "object",
            "required": ["prompt", "description", "subagent_type"],
            "properties": {
                "subagent_type": {
                    "type": "string",
                    "description": "Agent 类型名称。必须从工具描述中 `<available_subagent_types>` 段列出的清单中精确选择（builtin 如 `general-purpose`、`explore`，或已雇佣的数字员�� ID `emp-…`）。"
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
                    "description": "Agent 实例名。team_name 非空时必填（Teammate 派活）；异步子 Agent 也可选填以便 SendMessage 路由。"
                },
                "team_name": {
                    "type": "string",
                    "description": "目标 Team 名称。非空时将此 Agent 作为 Teammate 加入当前 Session 的 Team（Team 必须已通过 TeamCreate 创建）。此时 name 为必填。省略时始终按普通独立子 Agent 运行，不会因 active team 自动加入团队。"
                }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "TaskOutput",
            "【Support 工具】读取后台任务的 transcript 增量，支持 Agent(run_in_background=true) 与 Bash/PowerShell(run_in_background=true)。\
            \n\n用法：Agent({run_in_background: true, name: \"w1\"}) 立即返回 agent_id。\
            Bash/PowerShell({run_in_background: true}) 立即返回 task_id（task_type=local_bash）。\
            子 Agent 完成时通过 <task-notification> XML 通知（含 <output-file> 路径）。\
            期间或之后用 TaskOutput(task_id=..., offset=N) 读取产出。\
            \n\n交付规则：TaskOutput 只证明后台任务说了什么，不证明用户要求的文件、配置或数据已经存在。读取到完成消息后，仍要用文件读取、目录列举、测试或对应业务工具验证真实产物。\
            \n\n如果 transcript 显示任务失败、超时、依赖缺失或只做了分析，继续完成可独立推进的部分，并把阻塞原因写入要求的最终产物；不要只复述 transcript。若 transcript 停在“继续阅读/继续分析/准备写入”而没有真实产物，父 Agent 必须接管交付检查，优先读取目标路径、补写文件或记录阻塞。\
            \n\n不要用 TaskOutput 读取 Team/Teammate 成员发言；团队成员的对外发言只通过 SendMessage / peer-messages 进入主对话。\
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
                    "description": "后台任务 ID（Agent 返回 agent_id；Bash/PowerShell 返回 task_id）"
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
            "加载一个专项技能的详细指令作为内部参考。只用于动态上下文中已列出的 skill_id，不读取当前工作区文件。本地 `SKILL.md`、`.agents/skills/*/SKILL.md` 或 `skills/*/SKILL.md` 应使用 Glob/Read 发现和读取，不要调用 `Skill(skill_id=\"find-skills\")` 来查本地技能。技能正文中的输入文件、输出文件、禁止事项、验证命令和评分口径是本任务交付约束；调用后不要向用户说明内部能力选择过程，直接以业务语言承接用户需求。读取 Skill 后必须继续执行其中的方法：读取指定输入、运行 helper/脚本或用等价实现生成要求的文件，并验证输出；不要把“已读取技能”当作完成。",
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
                    "description": "技能 ID，必须来自动态上下文中的可用专项技能目录；不要传工作区里的 skills/<name> 目录名，本地 SKILL.md 应使用 Read 读取文件路径"
                }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "AskUserQuestion",
            "向用户提出结构化多选问题，等待用户回答后继续。\
            \n\n用途：收集用户偏好、澄清歧义、让用户在多个方案中选择。\
            \n\n每次调用支持 1-4 个问题，每个问题 2-4 个选项。\
            \n\n不要在 options 中添加“其他”“其它”“Other”“Other (please specify)”或任何同义的自定义回答选项；如果现有选项都不合适，用户界面会自己提供自定义输入入口。",
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
                                "description": "只填写具体、互斥的业务选项；不要添加“其他”“其它”“Other”或要求用户说明的兜底选项。",
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
            "创建一条持久化任务，用于当前 session/agent 工作清单。适用于多步骤、工具密集、跨轮、团队协作、后台任务或多个交付物容易遗漏的工作；纯聊天、快速事实问答、单步读取或一次性小修通常不要创建。若用户已经指定单个明确输出文件，且当前可以直接读取/计算/写入，先用 Read/Bash/PowerShell/Write/Edit 推进真实产物，不要把 TaskCreate 当作开场动作。若用户要求“不要创建其它文件/目录”“只输出这些文件”或工作区会被严格评分，不要用持久任务工具制造额外任务记录，除非用户明确要求可见任务管理；改用内部清单。任务清单只能辅助推进，不能替代用户要求的文件、配置、脚本、报告或数据产物。",
        )
        .with_kind(ToolKind::Support),
        json!({
            "type": "object",
            "required": ["subject", "description"],
            "properties": {
                "subject": { "type": "string", "description": "任务短标题，写完成条件而不是微小读取动作；保持 2-6 个粗粒度任务" },
                "description": { "type": "string", "description": "任务详细说明，应包含交付物、完成标准或验证方式；非平凡任务最后一项应是验证/交付检查" },
                "activeForm": { "type": "string", "description": "进行中展示文案，如 Running tests" },
                "metadata": { "type": "object", "description": "可选元数据" }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "TaskUpdate",
            "更新、删除或设置任务依赖、owner、status、metadata。只有在对应文件、工具调用、配置、测试、计算或阻塞记录真实完成后，才能把任务标为 completed；读过、开始写、正在分析、等待服务或需要继续都不能标 completed。对调度、排产、资源分配、权限分配、schema 输出或硬约束任务，completed 需要最终文件字段级断言通过，不能只凭文件存在、数量、目录列举或浅层解析。",
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
                "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "deleted"], "description": "completed 只用于真实生成/执行/验证成功，或阻塞原因已写入最终产物的任务；硬约束/调度/资源/schema 任务必须完成最终文件字段级断言后才能标 completed" },
                "owner": { "type": "string", "description": "任务 owner agent/name" },
                "addBlocks": { "type": "array", "items": { "type": "string" } },
                "addBlockedBy": { "type": "array", "items": { "type": "string" } },
                "metadata": { "type": "object", "description": "metadata merge；value=null 表示删除 key" }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "TaskList",
            "列出当前 task list 的所有任务及阻塞状态。最终回复前用它或内部清单对照原始请求，确认最后状态落到验证、交付或明确阻塞，而不是停在继续阅读/继续分析。",
        )
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
            "TaskClaim",
            "认领 owner 为 None 或 \"*\" 的任务，将 owner 设置为当前 agent。已被他人认领时拒绝。",
        )
        .with_kind(ToolKind::Support),
        json!({
            "type": "object",
            "required": ["taskId"],
            "properties": {
                "taskId": { "type": "string", "description": "要认领的任务 ID" }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "TaskStop",
            "终止一个正在后台运行的任务（Agent/Bash/PowerShell 均使用 task_id）。",
        )
        .with_kind(ToolKind::Support),
        json!({
            "type": "object",
            "required": ["task_id"],
            "properties": {
                "task_id": { "type": "string", "description": "后台任务 ID（Agent 返回 agent_id；Bash/PowerShell 返回 task_id）" },
                "task_type": {
                    "type": "string",
                    "enum": ["local_agent", "local_bash", "local_shell"],
                    "description": "可选任务类型。通常无需传；local_shell 是 local_bash 的兼容别名。"
                }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "TeamCreate",
            "把当前 session 升级为多 Agent Team 模式。何时调用：任务需要 ≥2 个独立 Worker 并行，或者跨 domain 协作。不该调用：纯聊天、1 步可完成、可以串行的工作。Team 最多 4 个 Teammate；当前 session 已经是 Team 时返回错误。调用后会注册 team-lead 名字，Teammate 可用 SendMessage(to: \"team-lead\") 寻址。",
        )
        .with_kind(ToolKind::Support),
        json!({
            "type": "object",
            "required": [],
            "properties": {
                "team_name": {
                    "type": "string",
                    "description": "Team 显示名，省略则用 team-{session8} 自动生成。"
                },
                "description": {
                    "type": "string",
                    "description": "Team 目标的一句话描述，便于审计。"
                }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "TeamDelete",
            "解散一个具名 team：取消该 team 内所有 Teammate 的 cancel token、移除 in-memory entry、标记 teams/{team_name}/config.json 为 deleted、清理三元 key 注册表。team_name 省略时使用当前 active team；team 不存在静默 noop。TeamDelete 是本轮 Team 编排的终止信号，成功后当前 Lead turn 会停止；不要在没有收到真实 Teammate 回复时调用它再继续用普通 Agent/Bash/Write 模拟专家讨论。",
        )
        .with_kind(ToolKind::Support),
        json!({
            "type": "object",
            "required": [],
            "properties": {
                "team_name": {
                    "type": "string",
                    "description": "要删除的 team 名称。省略则取当前 active team。"
                }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "TeamSwitch",
            "切换当前 conversation 的 active team。Lead 之后的 tool 调用（task / send_message 等）会路由到新 team 的目录。team 必须已存在（先通过 TeamCreate 创建）。一个 conversation 内同一时刻只有一个 active team，切换不删除老 team。",
        )
        .with_kind(ToolKind::Support),
        json!({
            "type": "object",
            "required": ["team_name"],
            "properties": {
                "team_name": {
                    "type": "string",
                    "description": "目标 team 名称（必须已通过 TeamCreate 创建）。"
                }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "TeammateStop",
            "强制取消一个 Teammate（Lead 紧急工具）。仅在以下情况使用：1) 已经向 Teammate 发了 shutdown_request 但未收到 shutdown_response（或 approve=false 且重试无果）；2) Teammate 卡死、行为异常；3) TeamDelete 之前的清理。普通收尾应优先用 shutdown_request 让 Teammate 自己 graceful 退出。幂等：取消已经退出/不存在的 agent 静默成功。",
        )
        .with_kind(ToolKind::Support),
        json!({
            "type": "object",
            "required": ["agent_name"],
            "properties": {
                "agent_name": { "type": "string", "description": "目标 Teammate 的 name（如 'researcher'）。" }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "SendMessage",
            "向同 session 的另一个 Agent 投递结构化消息。使用场景：Lead 给某 Teammate 派任务、Teammate 给 Lead 汇报阶段成果、Teammate 之间交接产出、广播紧急停止信号 (`to:\"*\"`)。不要用来报 task 状态（用 TaskUpdate）或写文件产物（直接写磁盘）。`to` 是 name（如 \"team-lead\" / \"researcher\" / \"*\" 广播）。`message` 是 StructuredMessage 5 个 variant 之一：text / shutdown_request / shutdown_response / plan_approval_request / plan_approval_response。",
        )
        .with_kind(ToolKind::Support),
        json!({
            "type": "object",
            "required": ["to", "message"],
            "properties": {
                "to": {
                    "type": "string",
                    "description": "目标 agent 的 name，或 \"*\" 表示广播到所有 Teammate（不含发送者自己）。"
                },
                "message": {
                    "type": "object",
                    "description": "StructuredMessage：{type:'text', content:'...'} 等 5 种 variant。",
                    "required": ["type"],
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": [
                                "text",
                                "shutdown_request",
                                "shutdown_response",
                                "plan_approval_request",
                                "plan_approval_response"
                            ]
                        }
                    }
                },
                "summary": {
                    "type": "string",
                    "description": "5-10 字 UI 预览文案（可选）。"
                }
            }
        }),
    ));

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

    // ── Primitive: agenda tools (spec §7) ──────────────────────────
    // organizer 强制为当前 persona——runtime 在 RequestScopedRuntimeDeps 注入 persona id，
    // 工具构造期绑死到 AgendaToolDeps，LLM 不传 organizer 字段。详见 spec §4.5。
    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "create_agenda_item",
            "【自用】为你（当前数字员工）自己创建一条到点自动触发的日程：一次性或循环（每天/每周/每月/每年），到点会以你（同一个 persona）的身份自动执行内置 prompt。",
        )
            .with_kind(ToolKind::Primitive)
            .with_read_only(false),
        json!({
            "type": "object",
            "required": ["title", "prompt", "start_at"],
            "properties": {
                "title": { "type": "string" },
                "prompt": { "type": "string", "description": "到点要执行的内容" },
                "start_at": { "type": "string", "format": "date-time" },
                "timezone": { "type": "string", "default": "Asia/Shanghai" },
                "rule": {
                    "type": "object",
                    "required": ["freq", "interval", "endCondition"],
                    "properties": {
                        "freq": { "type": "string", "enum": ["daily", "weekly", "monthly", "yearly"] },
                        "interval": { "type": "integer", "minimum": 1 },
                        "endCondition": {
                            "oneOf": [
                                { "type": "object", "required": ["kind"], "properties": { "kind": { "const": "never" } } },
                                { "type": "object", "required": ["kind", "n"], "properties": { "kind": { "const": "count" }, "n": { "type": "integer" } } },
                                { "type": "object", "required": ["kind", "at"], "properties": { "kind": { "const": "until" }, "at": { "type": "string", "format": "date-time" } } }
                            ]
                        }
                    }
                }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "list_agenda_items",
            "【自用】列出你（当前数字员工）自己的日程清单——你给自己设的循环任务、定时提醒。",
        )
            .with_kind(ToolKind::Primitive)
            .with_read_only(true),
        json!({
            "type": "object",
            "required": [],
            "properties": {
                "status_in": {
                    "type": "array",
                    "items": { "type": "string", "enum": ["active", "paused", "completed", "orphaned", "cancelled"] }
                },
                "limit": { "type": "integer", "default": 50 }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "update_agenda_item",
            "【自用】修改你自己创建的日程（标题/触发内容/频率/启用状态）。",
        )
        .with_kind(ToolKind::Primitive)
        .with_read_only(false),
        json!({
            "type": "object",
            "required": ["id"],
            "properties": {
                "id": { "type": "string" },
                "title": { "type": "string" },
                "prompt": { "type": "string" },
                "rule": {},
                "status": { "type": "string", "enum": ["active", "paused"] }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "cancel_agenda_item",
            "【自用】取消你自己创建的日程（软删除，可在 UI 恢复）。",
        )
        .with_kind(ToolKind::Primitive)
        .with_read_only(false)
        .with_destructive(true),
        json!({
            "type": "object",
            "required": ["id"],
            "properties": { "id": { "type": "string" } }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "skip_occurrence",
            "【自用】跳过你自己循环日程的某一次触发。",
        )
        .with_kind(ToolKind::Primitive)
        .with_read_only(false),
        json!({
            "type": "object",
            "required": ["id", "at"],
            "properties": {
                "id": { "type": "string" },
                "at": { "type": "string", "format": "date-time" }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "list_agenda_occurrences",
            "【自用】查看你自己日程过往的执行历史（成功/失败记录）。",
        )
        .with_kind(ToolKind::Primitive)
        .with_read_only(true),
        json!({
            "type": "object",
            "required": ["agenda_item_id"],
            "properties": {
                "agenda_item_id": { "type": "string" },
                "limit": { "type": "integer", "default": 20 }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "RefreshSkills",
            "通知 AIjia 重新扫描用户技能目录，让新装的技能立刻在对话和技能中心可见。\
             用法：刚通过 lotus_skill.py install 或别的方式装完技能后调用一次。\
             无参数。返回成功后下一 turn 的 catalog 含新技能。",
        )
        .with_kind(ToolKind::Support)
        .with_read_only(false),
        json!({
            "type": "object",
            "required": [],
            "properties": {}
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "SkillMarketSearch",
            "根据用户原始任务搜索企业技能市场，只返回少量候选技能。调用本工具前必须先调用 Skill({skill_id:\"find-skills\"}) 加载发现技能指令；用于当前已启用 skill catalog 没有明显覆盖专项任务时。普通公开网页、简单事实查询、闲聊或已启用技能明确覆盖的任务不要调用。",
        )
        .with_kind(ToolKind::Support)
        .with_read_only(true)
        .with_capability_scope(["network"])
        .with_max_result_size_chars(8_000),
        json!({
            "type": "object",
            "required": ["task"],
            "additionalProperties": false,
            "properties": {
                "task": {
                    "type": "string",
                    "description": "用户原始任务描述，例如：访问某网站抓取价格数据"
                },
                "capabilityHints": {
                    "type": "array",
                    "description": "可选能力提示。直接从用户原始任务中抽取少量业务系统名、业务对象、动作、文件/日志类型或分析目标；不要维护固定能力清单，也不要为了凑数填泛化词。如果用户明确是在处理普通公开网页，才使用 browser_automation 或 web_scraping；如果用户提到具名企业产品或业务系统，不要用泛化 browser/web_scraping 代替原词。",
                    "items": { "type": "string" },
                    "maxItems": 5
                },
                "maxResults": {
                    "type": "integer",
                    "description": "最多返回候选数量，默认 3，最大 5",
                    "minimum": 1,
                    "maximum": 5,
                    "default": 3
                }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "SkillMarketInstall",
            "安装 SkillMarketSearch 返回的市场技能。调用前必须确认 packageId 与 pluginId 来自搜索候选；如果同名技能已经安装，本工具只返回 alreadyInstalled；如果该技能已关闭，会提示不要重新安装或绕过关闭状态。",
        )
        .with_kind(ToolKind::Support)
        .with_destructive(true)
        .with_capability_scope(["network"])
        .with_default_timeout_secs(180)
        .with_max_result_size_chars(4_000),
        json!({
            "type": "object",
            "required": ["packageId", "pluginId"],
            "additionalProperties": false,
            "properties": {
                "packageId": {
                    "type": "integer",
                    "description": "SkillMarketSearch 返回的 packageId"
                },
                "pluginId": {
                    "type": "string",
                    "description": "SkillMarketSearch 返回的 pluginId / skill id"
                },
                "reason": {
                    "type": "string",
                    "description": "为什么安装这个技能，用于日志和调试"
                }
            }
        }),
    ));

    c
}

/// daily 模式允许 LLM 直接调用的跨平台逻辑工具集（Primitive + 必要 Support 工具）。
///
/// 对齐原子工具模型；register_runtime 注册的工具默认走 ToolDispatcher。
/// `Skill` 是例外：它需要 request-scoped SkillRegistry，但必须在 daily 模式可见。
///
/// 注意：Shell 工具需要按当前平台二次过滤。调用方给 LLM 或执行白名单使用时，
/// 必须走 [`daily_allowed_tools_for_current_platform`]，不能直接使用本常量。
pub const DAILY_ALLOWED_TOOLS: &[&str] = &[
    // Shell：每平台只注册其中一个（Unix=Bash, Windows=PowerShell）。
    "Bash",
    "PowerShell",
    "Read",
    "Write",
    "Edit",
    "Glob",
    "Grep",
    "WriteMemory",
    "SearchMemory",
    "WebSearch",
    "ImageTask",
    "Agent",
    "TaskOutput",
    "Skill",
    "AskUserQuestion",
    "TaskCreate",
    "TaskUpdate",
    "TaskList",
    "TaskGet",
    "TaskClaim",
    "TaskStop",
    "TeamCreate",
    "TeamDelete",
    "TeamSwitch",
    "TeammateStop",
    "SendMessage",
    // Agenda tools (spec §7) — request-scoped, organizer 由 runtime 注入
    "create_agenda_item",
    "list_agenda_items",
    "update_agenda_item",
    "cancel_agenda_item",
    "skip_occurrence",
    "list_agenda_occurrences",
    "RefreshSkills",
    "SkillMarketSearch",
    "SkillMarketInstall",
];

/// 某个工具在当前编译平台是否可执行。
pub fn tool_available_on_current_platform(tool_name: &str) -> bool {
    match tool_name {
        "Bash" => !cfg!(windows),
        "PowerShell" => cfg!(windows),
        _ => true,
    }
}

/// daily 模式当前平台真正允许暴露/执行的工具集。
pub fn daily_allowed_tools_for_current_platform() -> impl Iterator<Item = &'static str> {
    DAILY_ALLOWED_TOOLS
        .iter()
        .copied()
        .filter(|tool_name| tool_available_on_current_platform(tool_name))
}

/// 全局默认 catalog（延迟初始化）。
pub static TOOL_CATALOG: LazyLock<DynamicToolCatalog> =
    LazyLock::new(DynamicToolCatalog::new_with_defaults);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_allowed_tools_are_filtered_by_current_platform() {
        let allowed: Vec<&str> = daily_allowed_tools_for_current_platform().collect();

        if cfg!(windows) {
            assert!(!allowed.contains(&"Bash"));
            assert!(allowed.contains(&"PowerShell"));
        } else {
            assert!(allowed.contains(&"Bash"));
            assert!(!allowed.contains(&"PowerShell"));
        }
    }

    #[test]
    fn daily_allowed_tools_include_find_skills_market_tools() {
        let allowed: Vec<&str> = daily_allowed_tools_for_current_platform().collect();

        assert!(allowed.contains(&"SkillMarketSearch"));
        assert!(allowed.contains(&"SkillMarketInstall"));
    }

    #[test]
    fn skill_market_search_hints_use_task_terms_without_fixed_capability_examples() {
        let entry = TOOL_CATALOG
            .get_entry("SkillMarketSearch")
            .expect("SkillMarketSearch should be registered");
        let description = entry
            .json_schema
            .pointer("/properties/capabilityHints/description")
            .and_then(serde_json::Value::as_str)
            .expect("capabilityHints should have a description");

        assert!(description.contains("业务系统"));
        assert!(description.contains("业务对象"));
        assert!(description.contains("分析目标"));
        assert!(description.contains("不要维护固定能力清单"));
        assert!(description.contains("普通公开网页"));
        assert!(description.contains("browser_automation"));
        assert!(description.contains("web_scraping"));
        assert!(!description.contains(&format!("{}{}", "hr_", "system")));
        assert!(!description.contains(&format!("{}{}", "salary_", "analysis")));
        assert!(
            !description.contains("例如 browser_automation"),
            "generic browser hints must not be the primary examples: {description}"
        );
    }

    #[test]
    fn skill_market_search_description_keeps_scope_boundary() {
        let entry = TOOL_CATALOG
            .get_entry("SkillMarketSearch")
            .expect("SkillMarketSearch should be registered");
        let description = &entry.definition.description;

        assert!(description.contains("find-skills"));
        assert!(description.contains("专项任务"));
        assert!(description.contains("普通公开网页"));
        assert!(description.contains("已启用技能明确覆盖"));
        assert!(!description.contains("不要先问网址"));
        assert!(!description.contains("直接使用 browser"));
    }
}
