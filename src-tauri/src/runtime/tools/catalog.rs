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
        ToolDefinition::new("list_directory", "列出授权工作目录中的文件和子目录")
            .with_kind(ToolKind::Primitive)
            .with_read_only(true)
            .with_max_result_size_chars(4_000)
            .with_capability_scope(["workspace:read"]),
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "相对于授权工作目录的路径，默认 '.'", "default": "." }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new("read_workspace_file", "读取授权工作目录中的文本文件内容")
            .with_kind(ToolKind::Primitive)
            .with_read_only(true)
            .with_max_result_size_chars(16_000)
            .with_capability_scope(["workspace:read"]),
        json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": { "type": "string", "description": "相对于授权工作目录的文件路径" },
                "max_bytes": { "type": "integer", "description": "最多读取字节数，默认 1048576", "default": 1048576 }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new("search_files", "在授权工作目录中搜索匹配 glob 模式的文件")
            .with_kind(ToolKind::Primitive)
            .with_read_only(true)
            .with_max_result_size_chars(4_000)
            .with_capability_scope(["workspace:read"]),
        json!({
            "type": "object",
            "required": ["pattern"],
            "properties": {
                "pattern": { "type": "string", "description": "文件名 glob 模式，如 '*.csv'" },
                "path": { "type": "string", "description": "搜索的子目录（相对路径），默认 '.'", "default": "." },
                "max_results": { "type": "integer", "description": "最多返回结果数，默认 100", "default": 100 }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new("get_file_info", "获取授权工作目录中文件或目录的元数据")
            .with_kind(ToolKind::Primitive)
            .with_read_only(true)
            .with_capability_scope(["workspace:read"]),
        json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": { "type": "string", "description": "相对于授权工作目录的路径" }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new("write_file", "在授权工作目录中创建或覆盖写入文本文件")
            .with_kind(ToolKind::Primitive)
            .with_capability_scope(["workspace:write"]),
        json!({
            "type": "object",
            "required": ["path", "content"],
            "properties": {
                "path": { "type": "string", "description": "相对于授权工作目录的目标文件路径" },
                "content": { "type": "string", "description": "要写入的文件内容（UTF-8 文本）" }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "edit_file",
            "基于 old_string/new_string 精确替换编辑授权工作目录中的文件（要求 old_string 在文件中唯一存在）",
        )
        .with_kind(ToolKind::Primitive)
        .with_capability_scope(["workspace:read", "workspace:write"]),
        json!({
            "type": "object",
            "required": ["path", "old_string", "new_string"],
            "properties": {
                "path": { "type": "string", "description": "相对于授权工作目录的文件路径" },
                "old_string": {
                    "type": "string",
                    "description": "要替换的原始字符串，必须在文件中唯一存在。若为空字符串，则视为向空文件追加内容（文件必须为空或不存在）"
                },
                "new_string": { "type": "string", "description": "替换后的新字符串" }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "bash",
            "在授权工作目录中执行 shell 命令。默认 timeout 120s；当前前台路径在 timeout/cancel 时终止进程并返回错误。\
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
                "timeout_secs": {
                    "type": "integer",
                    "description": "超时秒数，默认 120，最大 600",
                    "default": 120
                }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "grep_content",
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

    c.insert(CatalogEntry::new(
        ToolDefinition::new("load_file",
            "加载已上传文件，使数据可在 execute_python 中以变量形式使用。\
            \n\n加载结果：单文件 → _df（DataFrame）或 _text（字符串）；\
            多文件场景下所有数据在 _dfs 字典（按 file_id 索引）或 _texts 字典中，_df/_text 指向最后加载的文件。\
            在 execute_python 中直接使用这些变量即可，禁止猜测文件路径。\
            \n\n_df 包含完整数据（非 sampleData 样本），分析时先用 len(_df) 确认规模，基于全量数据统计。\
            \n\n注意：Power 工具，执行 Python 解析、PII 脱敏、session 缓存写入等副作用。")
            .with_kind(ToolKind::Power)
            .with_default_timeout_secs(120)
            .with_capability_scope(["workspace:read", "workspace:write", "python:exec"]),
        json!({
            "type": "object",
            "required": ["file_id"],
            "properties": {
                "file_id": { "type": "string", "description": "已上传文件的 ID" },
                "sheet": { "type": "string", "description": "Excel 工作表名（可选）" },
                "nrows": { "type": "integer", "description": "最多加载行数（可选）" }
            }
        }),
    ));

    // ── Primitive: network ────────────────────────────────────────
    c.insert(CatalogEntry::new(
        ToolDefinition::new("web_search", "搜索互联网获取最新信息")
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

    // ── Primitive: browser ────────────────────────────────────────
    c.insert(CatalogEntry::new(
        ToolDefinition::new("browse_navigate", "导航浏览器到指定 URL")
            .with_kind(ToolKind::Primitive)
            .with_capability_scope(["browser"]),
        json!({
            "type": "object",
            "required": ["url"],
            "properties": {
                "url": { "type": "string", "description": "目标 URL" }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new("read_page_content", "读取当前浏览器页面的文本内容")
            .with_kind(ToolKind::Primitive)
            .with_capability_scope(["browser"]),
        json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS 选择器（可选，默认读取全页）" }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new("page_execute_js", "在当前浏览器页面执行 JavaScript")
            .with_kind(ToolKind::Primitive)
            .with_capability_scope(["browser"]),
        json!({
            "type": "object",
            "required": ["code"],
            "properties": {
                "code": { "type": "string", "description": "JavaScript 代码" }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new("extract_table_data", "从当前浏览器页面抽取表格数据")
            .with_kind(ToolKind::Primitive)
            .with_capability_scope(["browser"]),
        json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "表格 CSS 选择器" },
                "max_rows": { "type": "integer", "description": "最多抽取行数" }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new("extract_with_pagination", "分页抽取浏览器表格数据")
            .with_kind(ToolKind::Primitive)
            .with_capability_scope(["browser"]),
        json!({
            "type": "object",
            "required": ["next_selector"],
            "properties": {
                "next_selector": { "type": "string", "description": "下一页按钮 CSS 选择器" },
                "max_pages": { "type": "integer", "description": "最多翻页数", "default": 10 }
            }
        }),
    ));

    // ── Power: execute_python ─────────────────────────────────────
    c.insert(CatalogEntry::new(
        ToolDefinition::new("execute_python",
            "执行 Python 代码进行数据分析和文件处理。\
            \n\n【Python 环境】pandas(pd)、numpy(np)、scipy.stats 已预导入。\
            辅助函数：_print_table(headers, rows, title) 输出 Markdown 表格；\
            _export_detail(df, filename, title) 导出 Excel 并预览前 15 行；\
            _smart_read_csv(path) 自动检测编码。\
            工作目录为工作区根目录，各子目录：uploads/（上传文件）、exports/（导出数据）、reports/（报告）、charts/（图表）。\
            \n\n【数据来源】已上传文件先调用 load_file 加载，数据以 _df（单文件 DataFrame）/ _dfs（多文件 dict）/ _text / _texts 变量形式注入。\
            已连接本地目录先用 list_directory / search_files / read_workspace_file 读取后再传入本工具处理。\
            \n\n【文件管理函数】_ws_list(path, pattern) 列目录 | _ws_search(keyword) 搜内容 | _ws_info(path) 查详情 | _ws_convert(path, format) 格式转换 | _ws_merge(paths) 合并文件。\
            \n\n注意：Power 工具，有 session 状态和文件写出副作用。代码执行出错时直接修正重试。")
            .with_kind(ToolKind::Power)
            .with_max_result_size_chars(32_000)
            .with_preserve_tool_use_results(true)
            .with_default_timeout_secs(600)
            .with_capability_scope(["python:exec", "workspace:write"]),
        json!({
            "type": "object",
            "required": ["code"],
            "properties": {
                "code": { "type": "string", "description": "Python 代码" },
                "purpose": { "type": "string", "description": "简要说明代码用途" }
            }
        }),
    ));

    // ── Composite tools ───────────────────────────────────────────
    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "browse_data",
            "【Composite 工具】从内部业务系统抽取数据。\
            \n\n内部固定三步流程：\
            1. browse_and_extract(url) — 打开数据页面，查看表格和菜单；\
            2. extract_with_pagination() — 自动翻页提取全量数据并保存为 JSON；\
            3. 报告文件路径、总行数、列名。\
            \n\n返回文件路径，请用 execute_python 进一步处理。\
            ACCESS DENIED 时立即停止。一次只提取一个数据表。",
        )
        .with_kind(ToolKind::Composite)
        .with_capability_scope(["browser", "network", "workspace:write"]),
        json!({
            "type": "object",
            "required": ["task"],
            "properties": {
                "task": { "type": "string", "description": "需要抽取的数据描述" },
                "url": { "type": "string", "description": "目标系统 URL（可选）" }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "browse_and_extract",
            "【Composite 工具】导航到 URL 并抽取结构化数据（navigate + read + extract 三步合一）。\
            \n\n用于一次性提取页面数据。\
            如需分页全量抽取，改用 extract_with_pagination()，它自动处理分页且无需手动翻页参数。\
            禁止用 page_execute_js 提取表格数据——用本工具或 extract_table_data 替代。",
        )
        .with_kind(ToolKind::Composite)
        .with_capability_scope(["browser"]),
        json!({
            "type": "object",
            "required": ["url"],
            "properties": {
                "url": { "type": "string", "description": "目标 URL" },
                "selector": { "type": "string", "description": "数据选择器" }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new("generate_report",
            "【Composite 工具】生成专业分析报告（HTML/Markdown/PDF/DOCX）。\
            \n\n【数据传递规则】sections 参数禁止直接写入大段文本数据。\
            正确做法：先用 execute_python 从 _df 生成报告 sections 数据并写入 JSON 文件，\
            再调用 generate_report(source=\"文件路径\")。\
            \n\n内部包含：渲染 → 写文件 → 按需格式转换，多阶段操作。\
            用于分析末尾生成最终报告，不适合中间步骤。")
            .with_kind(ToolKind::Composite)
            .with_preserve_tool_use_results(true)
            .with_default_timeout_secs(300)
            .with_capability_scope(["workspace:write"]),
        json!({
            "type": "object",
            "required": ["title"],
            "properties": {
                "title": { "type": "string" },
                "sections": { "type": "array", "items": { "type": "object" } },
                "format": { "type": "string", "enum": ["html","markdown","pdf","docx"], "default": "html" }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new("generate_chart",
            "【Composite 工具】生成交互式数据可视化图表（计算 + 渲染 + 写文件）。\
            \n\n【数据传递规则】数据点超过 50 个时必须使用 data_file 参数（先用 execute_python 准备数据并写入 JSON 文件，再传入文件路径），\
            不得在 data 参数中直接内联大量数据点。")
            .with_kind(ToolKind::Composite)
            .with_default_timeout_secs(300)
            .with_capability_scope(["workspace:write"]),
        json!({
            "type": "object",
            "required": ["chart_type","title","data"],
            "properties": {
                "chart_type": { "type": "string", "enum": ["bar","line","scatter","box","heatmap","pie","histogram"] },
                "title": { "type": "string" },
                "data": { "type": "object" }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "export_data",
            "【Composite 工具】将数据导出为文件（数据转换 + 写文件）。\
            \n\n【使用方式】在 execute_python 中用 _export_detail(_df, filename, format) 直接导出，\
            禁止在 data 参数中传入原始数据数组（会超出 token 限制）。",
        )
        .with_kind(ToolKind::Composite)
        .with_capability_scope(["workspace:write"]),
        json!({
            "type": "object",
            "required": ["data","format","filename"],
            "properties": {
                "data": { "type": "object" },
                "format": { "type": "string", "enum": ["csv","excel","json"] },
                "filename": { "type": "string" }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "generate_slides",
            "【Composite 工具】生成演示文稿（多页渲染 + 写文件）。",
        )
        .with_kind(ToolKind::Composite)
        .with_capability_scope(["workspace:write"]),
        json!({
            "type": "object",
            "required": ["title"],
            "properties": {
                "title": { "type": "string" },
                "slides": { "type": "array" }
            }
        }),
    ));

    // ── Support tools ─────────────────────────────────────────────
    for (id, desc) in &[
        ("plan_update", "更新任务计划状态"),
        ("progress_update", "更新分析步骤进度"),
        ("save_analysis_note", "保存中间分析记录"),
    ] {
        c.insert(CatalogEntry::new(
            ToolDefinition::new(*id, *desc).with_kind(ToolKind::Support),
            json!({ "type": "object", "properties": {}, "required": [] }),
        ));
    }

    // ── Power: statistical analysis ───────────────────────────────
    c.insert(CatalogEntry::new(
        ToolDefinition::new("hypothesis_test", "统计假设检验（t-test/ANOVA/chi-square/Mann-Whitney/regression）")
            .with_kind(ToolKind::Power)
            .with_capability_scope(["python:exec"]),
        json!({
            "type": "object",
            "required": ["test_type", "groups"],
            "properties": {
                "test_type": { "type": "string", "enum": ["t_test", "anova", "chi_square", "regression", "mann_whitney"] },
                "groups": { "type": "array", "items": { "type": "string" }, "description": "要比较的列名" },
                "significance_level": { "type": "number", "default": 0.05 }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new("detect_anomalies", "检测数据中的异常值（Z-score/IQR/Grubbs）")
            .with_kind(ToolKind::Power)
            .with_capability_scope(["python:exec"]),
        json!({
            "type": "object",
            "required": ["column"],
            "properties": {
                "column": { "type": "string", "description": "要分析的列名" },
                "method": { "type": "string", "enum": ["zscore", "iqr", "grubbs"], "default": "zscore" },
                "threshold": { "type": "number" },
                "group_by": { "type": "string" }
            }
        }),
    ));

    c
}

/// daily 模式允许 LLM 直接调用的工具集。
///
/// 包含所有 Primitive + Power 工具，以及少数 daily 场景常用的 Composite 工具
/// （browse_data、generate_report、generate_chart、export_data）。
/// 不包含 browse_and_extract / generate_slides 等纯分析流程专属的 Composite 工具。
///
/// 对齐 claude-code-best 原子工具模型：只包含有 register_runtime 注册的 RuntimeTool。
///
/// 以下工具已移除（无 register_runtime 注册，或 ToolPlugin 路径已关闭）：
///   - web_search, browse_navigate, read_page_content（request-scoped，未全局注册）
///   - load_file, execute_python（request-scoped，未全局注册）
///   - browse_data, generate_report, generate_chart（request-scoped，未全局注册）
///   - export_data, plan_update, progress_update, save_analysis_note（ToolPlugin 已关闭）
///   - save_memory, search_memory（ToolPlugin 已关闭）
pub const DAILY_ALLOWED_TOOLS: &[&str] = &[
    // 以下 8 个工具均在 register_builtin_tools() 中 register_runtime 注册，走 ToolDispatcher
    "bash",
    "read_workspace_file",
    "write_file",
    "edit_file",
    "list_directory",
    "search_files",
    "get_file_info",
    "grep_content",
];

/// 全局默认 catalog（延迟初始化）。
pub static TOOL_CATALOG: LazyLock<DynamicToolCatalog> =
    LazyLock::new(DynamicToolCatalog::new_with_defaults);
