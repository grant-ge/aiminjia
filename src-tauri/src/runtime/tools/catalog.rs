//! ToolCatalog — 工具元数据的单一真相源。
//!
//! 所有工具（primitive/power/composite/support）都在此注册。
//! `llm/tools.rs` 中的旧 schema 定义降级为兼容层，不再新增。
//! `plugin/registry.rs` 的运行时注册以本 catalog 为权威来源。

use std::collections::HashMap;
use std::sync::LazyLock;

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
        Self { definition, json_schema }
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

fn build_default_catalog() -> ToolCatalog {
    let mut c = ToolCatalog { entries: HashMap::new() };

    // ── Primitive: workspace tools ──────────────────────────────────
    c.insert(CatalogEntry::new(
        ToolDefinition::new("list_directory", "列出授权工作目录中的文件和子目录")
            .with_kind(ToolKind::Primitive)
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
        ToolDefinition::new("load_file",
            "加载已上传文件，使数据可在 execute_python 中以变量形式使用。\
            \n\n加载结果：单文件 → _df（DataFrame）或 _text（字符串）；\
            多文件场景下所有数据在 _dfs 字典（按 file_id 索引）或 _texts 字典中，_df/_text 指向最后加载的文件。\
            在 execute_python 中直接使用这些变量即可，禁止猜测文件路径。\
            \n\n_df 包含完整数据（非 sampleData 样本），分析时先用 len(_df) 确认规模，基于全量数据统计。\
            \n\n注意：Power 工具，执行 Python 解析、PII 脱敏、session 缓存写入等副作用。")
            .with_kind(ToolKind::Power)
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
        ToolDefinition::new("browse_data",
            "【Composite 工具】从内部业务系统抽取数据。\
            内部会启动子代理，依次执行多步 browse_navigate/read_page_content/extract_table_data 操作，最终写出 JSON 文件。\
            返回文件路径，请用 execute_python 进一步处理。")
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
        ToolDefinition::new("browse_and_extract",
            "【Composite 工具】导航到 URL 并抽取结构化数据（navigate + read + extract 三步合一）。")
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
            内部包含：渲染 → 写文件 → 按需格式转换，多阶段操作。\
            用于分析末尾生成最终报告，不适合中间步骤。")
            .with_kind(ToolKind::Composite)
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
            "【Composite 工具】生成交互式数据可视化图表（计算 + 渲染 + 写文件）。")
            .with_kind(ToolKind::Composite)
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
        ToolDefinition::new("export_data",
            "【Composite 工具】将数据导出为文件（数据转换 + 写文件）。")
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
        ToolDefinition::new("generate_slides",
            "【Composite 工具】生成演示文稿（多页渲染 + 写文件）。")
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
        ("save_memory", "保存记忆条目"),
        ("search_memory", "搜索记忆"),
        ("core_memory", "读写核心记忆"),
        ("distill_memory", "蒸馏精简记忆"),
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
/// 需与 `tests/skill_tool_contract_test.rs` 中的 DAILY_ALLOWED_TOOLS 保持同步。
pub const DAILY_ALLOWED_TOOLS: &[&str] = &[
    // Primitive: workspace
    "list_directory",
    "read_workspace_file",
    "search_files",
    "get_file_info",
    // Primitive: network
    "web_search",
    // Primitive: browser
    "browse_navigate",
    "read_page_content",
    // Power
    "load_file",
    "execute_python",
    // Composite（daily 常用）
    "browse_data",
    "generate_report",
    "generate_chart",
    "export_data",
    // Support
    "plan_update",
    "progress_update",
    "save_analysis_note",
    "save_memory",
    "search_memory",
];

/// 全局默认 catalog（延迟初始化）。
pub static TOOL_CATALOG: LazyLock<ToolCatalog> = LazyLock::new(build_default_catalog);
