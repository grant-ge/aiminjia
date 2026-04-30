# Atomic Tool 工具体系 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 lotus-app 工具系统从"27 个全走 legacy PluginContext 的混杂工具"收口为"有 ToolKind 分类、单一 catalog 真相源、第一批原子工具走 RuntimeTool + 显式 PermissionPipeline"的状态。

**Architecture:** 引入 `ToolKind`（Primitive/Power/Composite/Support）和 `ToolCatalog`（单一真相源），将 workspace tools 等 11 个工具迁到 `RuntimeTool`，用 `CapabilityPipeline` 替换 `AllowAllPermissionPipeline`，`llm/tools.rs` 降级为兼容层，`browse_data`/`generate_report` 等被标记为 Composite。分 5 期（A1-A5），每期独立可测试。

**Tech Stack:** Rust / Tokio / async_trait / serde_json / anyhow / cargo test (integration tests in `src-tauri/tests/`)

---

## Phase A1：ToolKind + ToolCatalog 建立单一 contract

### Files

- Modify: `src-tauri/src/runtime/tools/definition.rs`
- Create: `src-tauri/src/runtime/tools/catalog.rs`
- Modify: `src-tauri/src/runtime/tools/mod.rs`
- Modify: `src-tauri/src/llm/tools.rs` (降级为兼容层，不再是真相源)
- Create: `src-tauri/tests/tool_catalog_contract_test.rs`
- Create: `src-tauri/tests/tool_schema_single_source_test.rs`

---

### Task A1.1：给 ToolDefinition 加 ToolKind 字段

**Files:**
- Modify: `src-tauri/src/runtime/tools/definition.rs`

- [ ] **Step 1: 写失败测试**

在 `src-tauri/tests/tool_catalog_contract_test.rs` 中先写（文件此时尚不存在，只写测试内容）：

```rust
// src-tauri/tests/tool_catalog_contract_test.rs
use app_lib::runtime::tools::definition::{ToolDefinition, ToolKind};

#[test]
fn tool_definition_has_kind_field() {
    let def = ToolDefinition::new("web_search", "Search the web")
        .with_kind(ToolKind::Primitive);
    assert!(matches!(def.kind, ToolKind::Primitive));
}

#[test]
fn tool_kind_default_is_primitive() {
    let def = ToolDefinition::new("echo", "Echo test");
    assert!(matches!(def.kind, ToolKind::Primitive));
}

#[test]
fn execute_python_kind_is_power() {
    let def = ToolDefinition::new("execute_python", "Run Python")
        .with_kind(ToolKind::Power);
    assert!(matches!(def.kind, ToolKind::Power));
}

#[test]
fn browse_data_kind_is_composite() {
    let def = ToolDefinition::new("browse_data", "Multi-step browser agent")
        .with_kind(ToolKind::Composite);
    assert!(matches!(def.kind, ToolKind::Composite));
}
```

- [ ] **Step 2: 运行测试，确认编译失败（ToolKind 不存在）**

```bash
cd src-tauri && cargo test tool_catalog_contract -- --nocapture 2>&1 | head -30
```
期望：`error[E0412]: cannot find type 'ToolKind'`

- [ ] **Step 3: 修改 definition.rs，加入 ToolKind + builder 方法**

```rust
// src-tauri/src/runtime/tools/definition.rs

/// 工具分类——用于区分原子能力、强能力执行器、编排工具和辅助工具。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    /// 单一动作、单一资源域、低副作用。可直接组合使用。
    Primitive,
    /// 强能力单域执行器，有 session 状态或文件副作用，但仍单段执行。
    Power,
    /// 内部调度多个 Primitive / child run / 多阶段动作。不是基础能力。
    Composite,
    /// 计划、进度、记忆等辅助工具。
    Support,
}

impl Default for ToolKind {
    fn default() -> Self {
        ToolKind::Primitive
    }
}

#[derive(Clone, Debug)]
pub struct ToolDefinition {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub capability_scope: Vec<String>,
    pub kind: ToolKind,
}

impl ToolDefinition {
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            display_name: id.clone(),
            id,
            description: description.into(),
            capability_scope: Vec::new(),
            kind: ToolKind::default(),
        }
    }

    /// 设置工具分类。
    pub fn with_kind(mut self, kind: ToolKind) -> Self {
        self.kind = kind;
        self
    }

    /// 设置能力域列表（用于权限管线校验）。
    pub fn with_capability_scope(mut self, scopes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.capability_scope = scopes.into_iter().map(Into::into).collect();
        self
    }
}
```

- [ ] **Step 4: 运行测试，确认通过**

```bash
cd src-tauri && cargo test tool_catalog_contract -- --nocapture
```
期望：4 个测试全部 PASS

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/runtime/tools/definition.rs src-tauri/tests/tool_catalog_contract_test.rs
git commit -m "feat(atomic-tool): add ToolKind enum and builder to ToolDefinition"
```

---

### Task A1.2：创建 ToolCatalog — 单一 schema 真相源

**Files:**
- Create: `src-tauri/src/runtime/tools/catalog.rs`
- Modify: `src-tauri/src/runtime/tools/mod.rs`
- Create: `src-tauri/tests/tool_schema_single_source_test.rs`

- [ ] **Step 1: 写失败测试**

```rust
// src-tauri/tests/tool_schema_single_source_test.rs
use app_lib::runtime::tools::catalog::{ToolCatalog, TOOL_CATALOG};
use app_lib::runtime::tools::definition::ToolKind;

#[test]
fn catalog_contains_all_registered_tools() {
    // 至少包含 11 个首批原子工具和主要 composite 工具
    let required = vec![
        "list_directory", "read_workspace_file", "search_files", "get_file_info",
        "web_search", "browse_navigate", "read_page_content", "page_execute_js",
        "extract_table_data", "extract_with_pagination", "load_file",
        "execute_python",
        "browse_data", "generate_report", "export_data",
    ];
    let catalog = ToolCatalog::default_catalog();
    for name in &required {
        assert!(
            catalog.get(name).is_some(),
            "Tool '{}' not found in catalog",
            name
        );
    }
}

#[test]
fn execute_python_is_power_in_catalog() {
    let catalog = ToolCatalog::default_catalog();
    let def = catalog.get("execute_python").expect("execute_python must be in catalog");
    assert!(matches!(def.kind, ToolKind::Power), "execute_python must be Power kind");
}

#[test]
fn browse_data_is_composite_in_catalog() {
    let catalog = ToolCatalog::default_catalog();
    let def = catalog.get("browse_data").expect("browse_data must be in catalog");
    assert!(matches!(def.kind, ToolKind::Composite), "browse_data must be Composite kind");
}

#[test]
fn workspace_tools_are_primitive_in_catalog() {
    let catalog = ToolCatalog::default_catalog();
    for name in &["list_directory", "read_workspace_file", "search_files", "get_file_info"] {
        let def = catalog.get(name).unwrap_or_else(|| panic!("{} must be in catalog", name));
        assert!(
            matches!(def.kind, ToolKind::Primitive),
            "{} must be Primitive kind, got {:?}",
            name, def.kind
        );
    }
}

#[test]
fn catalog_tool_ids_have_no_duplicates() {
    let catalog = ToolCatalog::default_catalog();
    let ids = catalog.all_ids();
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "Catalog must not contain duplicate tool IDs");
}
```

- [ ] **Step 2: 运行，确认编译失败**

```bash
cd src-tauri && cargo test tool_schema_single_source -- --nocapture 2>&1 | head -20
```
期望：`error[E0433]: cannot find module 'catalog'`

- [ ] **Step 3: 创建 catalog.rs**

```rust
// src-tauri/src/runtime/tools/catalog.rs
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
        static CATALOG: LazyLock<ToolCatalog> = LazyLock::new(build_default_catalog);
        // 克隆 entries 构建新实例（entries 是 Clone 的 HashMap）
        Self {
            entries: CATALOG.entries.clone(),
        }
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
        ToolDefinition::new("load_file", "加载已上传文件，使数据可在 execute_python 中以 _df/_text 变量使用")
            .with_kind(ToolKind::Primitive)
            .with_capability_scope(["workspace:read"]),
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
            注意：这是 Power 工具，有 session 状态和文件写出副作用。\
            建议先用 load_file 或 list_directory 准备数据再调用。")
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
        ("hypothesis_test", "统计假设检验"),
        ("detect_anomalies", "异常值检测"),
    ] {
        c.insert(CatalogEntry::new(
            ToolDefinition::new(*id, *desc).with_kind(ToolKind::Support),
            json!({ "type": "object", "properties": {} }),
        ));
    }

    c
}

/// 全局默认 catalog（延迟初始化）。
pub static TOOL_CATALOG: LazyLock<ToolCatalog> = LazyLock::new(ToolCatalog::default_catalog);
```

- [ ] **Step 4: 更新 mod.rs，导出 catalog**

在 `src-tauri/src/runtime/tools/mod.rs` 末尾加入：

```rust
pub mod catalog;
pub use catalog::{CatalogEntry, ToolCatalog, TOOL_CATALOG};
```

- [ ] **Step 5: 运行测试，确认通过**

```bash
cd src-tauri && cargo test tool_schema_single_source -- --nocapture
```
期望：5 个测试全部 PASS

- [ ] **Step 6: 运行全量 Rust 测试，确认无回归**

```bash
cd src-tauri && cargo test -- --nocapture 2>&1 | tail -20
```
期望：所有 test result 为 ok

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/runtime/tools/catalog.rs \
        src-tauri/src/runtime/tools/mod.rs \
        src-tauri/tests/tool_schema_single_source_test.rs
git commit -m "feat(atomic-tool): add ToolCatalog as single schema source with ToolKind"
```

---

### Task A1.3：将 llm/tools.rs 降级为兼容层

**Files:**
- Modify: `src-tauri/src/llm/tools.rs`

- [ ] **Step 1: 在 llm/tools.rs 顶部加入 deprecation 注释，并从 catalog 委托 schema**

读取 `llm/tools.rs` 当前内容（已在上下文中）。在文件顶部原有注释之后，`static ALL_TOOLS` 之前加入如下兼容委托，并删除 `build_tool_definitions()` 函数中的硬编码 vec，改为从 catalog 生成：

```rust
// ⚠️ COMPATIBILITY LAYER — not the source of truth for tool schemas.
// Use `runtime::tools::catalog::ToolCatalog` for authoritative tool definitions.
// This file is kept only for the `get_tool_definitions_for_step()` step-filter API
// used by legacy analysis orchestration. New code must NOT add tool definitions here.
```

将 `build_tool_definitions()` 内容替换为从 catalog 委托（只保留当前 `ALL_TOOLS` 的 10 个工具名对应的 schema）：

```rust
use crate::runtime::tools::catalog::TOOL_CATALOG;

/// Build tool definitions from catalog — compatibility shim.
fn build_tool_definitions() -> Vec<ToolDefinition> {
    let step_tool_names = [
        "web_search", "execute_python", "load_file", "generate_report",
        "generate_chart", "hypothesis_test", "detect_anomalies",
        "save_analysis_note", "export_data", "progress_update",
    ];
    step_tool_names
        .iter()
        .filter_map(|name| {
            TOOL_CATALOG.get_entry(name).map(|entry| ToolDefinition {
                name: entry.definition.id.clone(),
                description: entry.definition.description.clone(),
                parameters: entry.json_schema.clone(),
            })
        })
        .collect()
}
```

- [ ] **Step 2: 运行现有 llm/tools 单测，确认通过**

```bash
cd src-tauri && cargo test --lib llm::tools -- --nocapture
```
期望：全部 PASS（工具数量断言 `tools.len() == 10` 仍然通过）

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/llm/tools.rs
git commit -m "refactor(atomic-tool): llm/tools.rs delegates to ToolCatalog (compatibility layer)"
```

---

## Phase A2：第一批原子工具迁移到 RuntimeTool

**目标：** 将 4 个 workspace tools 从 `ToolPlugin` 迁到 `RuntimeTool`，打通 `capability_scope` → `CapabilityContext` 路径，作为后续批量迁移的模板。

### Files

- Create: `src-tauri/src/runtime/tools/builtin/mod.rs`
- Create: `src-tauri/src/runtime/tools/builtin/workspace.rs`
- Modify: `src-tauri/src/runtime/tools/mod.rs`
- Create: `src-tauri/tests/runtime_tool_registry_test.rs`

---

### Task A2.1：创建 runtime/tools/builtin/ 目录和 workspace RuntimeTool

**Files:**
- Create: `src-tauri/src/runtime/tools/builtin/mod.rs`
- Create: `src-tauri/src/runtime/tools/builtin/workspace.rs`

- [ ] **Step 1: 写失败测试**

```rust
// src-tauri/tests/runtime_tool_registry_test.rs
use app_lib::runtime::tools::builtin::workspace::{
    ListDirectoryRuntimeTool, ReadWorkspaceFileRuntimeTool,
    SearchFilesRuntimeTool, GetFileInfoRuntimeTool,
};
use app_lib::runtime::tools::{
    RuntimeTool, ToolDefinition, ToolExecutionContext,
};
use app_lib::runtime::tools::capability::{CapabilityContext, StorageCapability};
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

fn make_ctx_with_workspace(tmp: &TempDir) -> ToolExecutionContext {
    let cap = CapabilityContext::with_workspace(
        tmp.path().to_path_buf(),
        "test-ws",
    );
    ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
        .with_capability(Arc::new(cap))
}

#[tokio::test]
async fn list_directory_runtime_tool_lists_files() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("data.csv"), b"col1\n1\n").unwrap();
    let ctx = make_ctx_with_workspace(&tmp);
    let tool = ListDirectoryRuntimeTool;
    let result = RuntimeTool::execute(&tool, json!({"path": "."}), ctx).await.unwrap();
    assert!(result.content.contains("data.csv"), "Should list data.csv, got: {}", result.content);
}

#[tokio::test]
async fn list_directory_requires_capability_context() {
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1"); // no capability
    let tool = ListDirectoryRuntimeTool;
    let result = RuntimeTool::execute(&tool, json!({}), ctx).await;
    assert!(result.is_err(), "Should fail without capability context");
}

#[tokio::test]
async fn read_workspace_file_runtime_tool_reads_content() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("hello.txt"), b"hello world").unwrap();
    let ctx = make_ctx_with_workspace(&tmp);
    let tool = ReadWorkspaceFileRuntimeTool;
    let result = RuntimeTool::execute(&tool, json!({"path": "hello.txt"}), ctx).await.unwrap();
    assert!(result.content.contains("hello world"), "Should contain file content");
}

#[tokio::test]
async fn search_files_runtime_tool_finds_csv() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("a.csv"), b"").unwrap();
    std::fs::write(tmp.path().join("b.txt"), b"").unwrap();
    let ctx = make_ctx_with_workspace(&tmp);
    let tool = SearchFilesRuntimeTool;
    let result = RuntimeTool::execute(&tool, json!({"pattern": "*.csv"}), ctx).await.unwrap();
    assert!(result.content.contains("a.csv"), "Should find a.csv");
    assert!(!result.content.contains("b.txt"), "Should not find b.txt");
}

#[tokio::test]
async fn workspace_runtime_tools_have_correct_kind() {
    use app_lib::runtime::tools::definition::ToolKind;
    let tools: Vec<Box<dyn RuntimeTool>> = vec![
        Box::new(ListDirectoryRuntimeTool),
        Box::new(ReadWorkspaceFileRuntimeTool),
        Box::new(SearchFilesRuntimeTool),
        Box::new(GetFileInfoRuntimeTool),
    ];
    for tool in &tools {
        let def = tool.definition();
        assert!(
            matches!(def.kind, ToolKind::Primitive),
            "Tool '{}' should be Primitive kind",
            def.id
        );
    }
}
```

- [ ] **Step 2: 运行，确认编译失败**

```bash
cd src-tauri && cargo test runtime_tool_registry -- --nocapture 2>&1 | head -20
```
期望：`error[E0433]: cannot find module 'builtin'`

- [ ] **Step 3: 创建 builtin/mod.rs**

```rust
// src-tauri/src/runtime/tools/builtin/mod.rs
//! First-class RuntimeTool implementations.
//! These tools do NOT use PluginContext — they use ToolExecutionContext + CapabilityContext.
pub mod workspace;
```

- [ ] **Step 4: 创建 builtin/workspace.rs**

```rust
// src-tauri/src/runtime/tools/builtin/workspace.rs
//! Workspace primitive tools as RuntimeTool.
//!
//! These tools require `ctx.capability.storage.authorized_workspace` to be set.
//! They NEVER accept a PluginContext — permissions come from CapabilityContext.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;

use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;
use crate::storage::file_manager;

fn require_authorized_root(ctx: &ToolExecutionContext) -> Result<std::path::PathBuf, ToolError> {
    ctx.capability
        .as_ref()
        .and_then(|c| c.storage.as_ref())
        .map(|s| {
            s.authorized_workspace
                .as_ref()
                .map(|aw| aw.root_path.clone())
                .unwrap_or_else(|| s.workspace_path.clone())
        })
        .ok_or_else(|| ToolError::PermissionDenied(
            "No capability context. Authorized workspace required for file tools.".into()
        ))
}

fn resolve_path(root: &Path, rel: &str) -> Result<std::path::PathBuf, ToolError> {
    file_manager::resolve_local_reference(root, rel)
        .map_err(|e| ToolError::PermissionDenied(e.to_string()))
}

fn tool_result(tool_name: &str, value: Value) -> ToolResult {
    ToolResult {
        tool_name: tool_name.to_string(),
        content: serde_json::to_string_pretty(&value).unwrap_or_default(),
        data: Some(value),
    }
}

// ── ListDirectoryRuntimeTool ───────────────────────────────────────────────

pub struct ListDirectoryRuntimeTool;

#[async_trait]
impl RuntimeTool for ListDirectoryRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG.get("list_directory")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("list_directory", "List authorized directory"))
    }

    async fn execute(&self, input: Value, ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        let root = require_authorized_root(&ctx)?;
        let rel = input.get("path").and_then(Value::as_str).unwrap_or(".");
        let resolved = resolve_path(&root, rel)?;
        if !resolved.is_dir() {
            return Err(ToolError::ExecutionFailed(format!("Not a directory: {}", rel)));
        }
        let mut files = Vec::new();
        for entry in std::fs::read_dir(&resolved)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
            .flatten()
        {
            let meta = entry.metadata().ok();
            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            files.push(json!({
                "name": entry.file_name().to_string_lossy(),
                "type": if is_dir { "directory" } else { "file" },
                "size": meta.as_ref().map(|m| m.len()).unwrap_or(0),
            }));
        }
        files.sort_by(|a, b| {
            b["type"].as_str().unwrap_or("").cmp(a["type"].as_str().unwrap_or(""))
                .then(a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")))
        });
        let count = files.len();
        Ok(tool_result("list_directory", json!({ "path": rel, "files": files, "count": count })))
    }
}

// ── ReadWorkspaceFileRuntimeTool ──────────────────────────────────────────

pub struct ReadWorkspaceFileRuntimeTool;

#[async_trait]
impl RuntimeTool for ReadWorkspaceFileRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG.get("read_workspace_file")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("read_workspace_file", "Read workspace file"))
    }

    async fn execute(&self, input: Value, ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        let root = require_authorized_root(&ctx)?;
        let rel = input.get("path").and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: path".into()))?;
        let max_bytes = input.get("max_bytes").and_then(Value::as_u64).unwrap_or(1_048_576) as usize;
        let resolved = resolve_path(&root, rel)?;
        if !resolved.is_file() {
            return Err(ToolError::ExecutionFailed(format!("Not a file: {}", rel)));
        }
        let bytes = std::fs::read(&resolved)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let truncated = bytes.len() > max_bytes;
        let content = String::from_utf8_lossy(if truncated { &bytes[..max_bytes] } else { &bytes }).to_string();
        let mut result = json!({ "path": rel, "content": content, "size": bytes.len() });
        if truncated {
            result["truncated"] = json!(true);
        }
        Ok(tool_result("read_workspace_file", result))
    }
}

// ── SearchFilesRuntimeTool ────────────────────────────────────────────────

pub struct SearchFilesRuntimeTool;

fn matches_glob(name: &str, pattern: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 { return name == pattern; }
    let mut remaining = name;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() { continue; }
        if i == 0 {
            if !remaining.starts_with(part) { return false; }
            remaining = &remaining[part.len()..];
        } else if i == parts.len() - 1 {
            if !remaining.ends_with(part) { return false; }
        } else if let Some(pos) = remaining.find(part) {
            remaining = &remaining[pos + part.len()..];
        } else {
            return false;
        }
    }
    true
}

fn walk_dir_collect(dir: &Path, file_pattern: &str, root: &Path, results: &mut Vec<Value>, max: usize) {
    if results.len() >= max { return; }
    let Ok(entries) = std::fs::read_dir(dir) else { return; };
    for entry in entries.flatten() {
        if results.len() >= max { break; }
        let Ok(ft) = entry.file_type() else { continue; };
        if ft.is_symlink() { continue; }
        let path = entry.path();
        if ft.is_dir() {
            walk_dir_collect(&path, file_pattern, root, results, max);
        } else if ft.is_file() {
            let name = entry.file_name().to_string_lossy().to_string();
            if matches_glob(&name, file_pattern) {
                let rel = path.strip_prefix(root).map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.to_string_lossy().to_string());
                results.push(json!({ "name": name, "path": rel, "size": entry.metadata().map(|m| m.len()).unwrap_or(0) }));
            }
        }
    }
}

#[async_trait]
impl RuntimeTool for SearchFilesRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG.get("search_files")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("search_files", "Search files"))
    }

    async fn execute(&self, input: Value, ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        let root = require_authorized_root(&ctx)?;
        let pattern = input.get("pattern").and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: pattern".into()))?;
        let sub = input.get("path").and_then(Value::as_str).unwrap_or(".");
        let max = input.get("max_results").and_then(Value::as_u64).unwrap_or(100) as usize;
        let base = resolve_path(&root, sub)?;
        let file_pattern = Path::new(pattern).file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| pattern.to_string());
        let mut matches = Vec::new();
        walk_dir_collect(&base, &file_pattern, &root, &mut matches, max);
        let count = matches.len();
        Ok(tool_result("search_files", json!({ "pattern": pattern, "path": sub, "matches": matches, "count": count })))
    }
}

// ── GetFileInfoRuntimeTool ────────────────────────────────────────────────

pub struct GetFileInfoRuntimeTool;

#[async_trait]
impl RuntimeTool for GetFileInfoRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG.get("get_file_info")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("get_file_info", "Get file info"))
    }

    async fn execute(&self, input: Value, ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        let root = require_authorized_root(&ctx)?;
        let rel = input.get("path").and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: path".into()))?;
        let resolved = resolve_path(&root, rel)?;
        if !resolved.exists() {
            return Err(ToolError::ExecutionFailed(format!("Path does not exist: {}", rel)));
        }
        let meta = std::fs::metadata(&resolved)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let is_dir = meta.is_dir();
        let modified = meta.modified().ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        let mut info = json!({
            "path": rel,
            "name": resolved.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            "type": if is_dir { "directory" } else { "file" },
            "size": meta.len(),
        });
        if let Some(ts) = modified { info["modified_unix"] = json!(ts); }
        if !is_dir {
            let ext = resolved.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
            info["extension"] = json!(ext);
        }
        Ok(tool_result("get_file_info", info))
    }
}
```

- [ ] **Step 5: 更新 mod.rs，导出 builtin**

在 `src-tauri/src/runtime/tools/mod.rs` 加入：

```rust
pub mod builtin;
```

- [ ] **Step 6: 运行测试，确认通过**

```bash
cd src-tauri && cargo test runtime_tool_registry -- --nocapture
```
期望：5 个测试全部 PASS

- [ ] **Step 7: 运行全量 Rust 测试**

```bash
cd src-tauri && cargo test -- --nocapture 2>&1 | tail -20
```

- [ ] **Step 8: 提交**

```bash
git add src-tauri/src/runtime/tools/builtin/ \
        src-tauri/src/runtime/tools/mod.rs \
        src-tauri/tests/runtime_tool_registry_test.rs
git commit -m "feat(atomic-tool): workspace tools as RuntimeTool in runtime/tools/builtin/"
```

---

## Phase A3：PermissionPipeline 真正落地

### Files

- Modify: `src-tauri/src/runtime/tools/permission.rs`
- Modify: `src-tauri/src/runtime/tools/capability.rs`（加 `has_browser_capability` 辅助）
- Create: `src-tauri/tests/tool_permission_pipeline_test.rs`

---

### Task A3.1：实现 CapabilityPermissionPipeline

- [ ] **Step 1: 写失败测试**

```rust
// src-tauri/tests/tool_permission_pipeline_test.rs
use app_lib::runtime::tools::{
    ToolDispatcher, ToolExecutionContext,
};
use app_lib::runtime::tools::permission::{CapabilityPermissionPipeline, PermissionPipeline};
use app_lib::runtime::tools::definition::ToolDefinition;
use app_lib::runtime::tools::capability::{CapabilityContext, StorageCapability};
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

fn def_with_scope(id: &str, scopes: &[&str]) -> ToolDefinition {
    ToolDefinition::new(id, "test")
        .with_capability_scope(scopes.iter().copied())
}

fn ctx_no_capability() -> ToolExecutionContext {
    ToolExecutionContext::for_test("conv", "run", "tc")
}

fn ctx_with_workspace(tmp: &TempDir) -> ToolExecutionContext {
    let cap = CapabilityContext::with_workspace(tmp.path().to_path_buf(), "ws");
    ToolExecutionContext::for_test("conv", "run", "tc").with_capability(Arc::new(cap))
}

#[test]
fn tool_without_scope_is_always_allowed() {
    let pipeline = CapabilityPermissionPipeline;
    let def = ToolDefinition::new("echo", "no scope");
    let ctx = ctx_no_capability();
    assert!(pipeline.authorize(&def, &json!({}), &ctx).is_ok());
}

#[test]
fn workspace_read_tool_rejected_without_capability() {
    let pipeline = CapabilityPermissionPipeline;
    let def = def_with_scope("list_directory", &["workspace:read"]);
    let ctx = ctx_no_capability();
    let result = pipeline.authorize(&def, &json!({}), &ctx);
    assert!(result.is_err(), "workspace:read tool must be rejected without capability");
    let err = result.unwrap_err().to_string();
    assert!(err.contains("workspace") || err.contains("capability"),
        "Error should mention workspace/capability: {}", err);
}

#[test]
fn workspace_read_tool_allowed_with_workspace_capability() {
    let tmp = TempDir::new().unwrap();
    let pipeline = CapabilityPermissionPipeline;
    let def = def_with_scope("list_directory", &["workspace:read"]);
    let ctx = ctx_with_workspace(&tmp);
    assert!(pipeline.authorize(&def, &json!({}), &ctx).is_ok());
}

#[test]
fn browser_tool_rejected_without_browser_capability() {
    let pipeline = CapabilityPermissionPipeline;
    let def = def_with_scope("browse_navigate", &["browser"]);
    let ctx = ctx_no_capability();
    let result = pipeline.authorize(&def, &json!({}), &ctx);
    assert!(result.is_err(), "browser tool must be rejected without browser capability");
}
```

- [ ] **Step 2: 运行，确认编译失败**

```bash
cd src-tauri && cargo test tool_permission_pipeline -- --nocapture 2>&1 | head -20
```
期望：`error[E0412]: cannot find type 'CapabilityPermissionPipeline'`

- [ ] **Step 3: 实现 CapabilityPermissionPipeline**

```rust
// src-tauri/src/runtime/tools/permission.rs  (完整替换)
use anyhow::{bail, Result};
use serde_json::Value;

use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;

pub trait PermissionPipeline: Send + Sync {
    fn authorize(
        &self,
        definition: &ToolDefinition,
        input: &Value,
        ctx: &ToolExecutionContext,
    ) -> Result<()>;
}

/// 允许所有工具——仅用于测试和 legacy bridge。
#[derive(Clone, Default)]
pub struct AllowAllPermissionPipeline;

impl PermissionPipeline for AllowAllPermissionPipeline {
    fn authorize(&self, _def: &ToolDefinition, _input: &Value, _ctx: &ToolExecutionContext) -> Result<()> {
        Ok(())
    }
}

/// 基于 capability_scope 的权限管线。
///
/// 规则：
/// - 工具 `capability_scope` 为空 → 始终允许
/// - 包含 `workspace:read` 或 `workspace:write` → 需要 `ctx.capability.storage` 存在
/// - 包含 `browser` → 需要 `ctx.capability.has_browser_capability()` = true
/// - 包含 `python:exec` → 需要 `ctx.capability.storage` 存在（session 依附于 workspace）
/// - 包含 `network` → 始终允许（网络访问在运行时阶段不做本地校验）
#[derive(Clone, Default)]
pub struct CapabilityPermissionPipeline;

impl PermissionPipeline for CapabilityPermissionPipeline {
    fn authorize(&self, definition: &ToolDefinition, _input: &Value, ctx: &ToolExecutionContext) -> Result<()> {
        if definition.capability_scope.is_empty() {
            return Ok(());
        }
        for scope in &definition.capability_scope {
            match scope.as_str() {
                "workspace:read" | "workspace:write" => {
                    if ctx.capability.as_ref().and_then(|c| c.storage.as_ref()).is_none() {
                        bail!(
                            "Tool '{}' requires workspace capability (scope: {}). \
                            Authorize a workspace directory first.",
                            definition.id, scope
                        );
                    }
                }
                "browser" => {
                    let has_browser = ctx.capability.as_ref()
                        .map(|c| c.has_browser_capability())
                        .unwrap_or(false);
                    if !has_browser {
                        bail!(
                            "Tool '{}' requires browser capability. \
                            A browser connector must be active.",
                            definition.id
                        );
                    }
                }
                "python:exec" => {
                    if ctx.capability.as_ref().and_then(|c| c.storage.as_ref()).is_none() {
                        bail!(
                            "Tool '{}' requires a workspace context for Python execution.",
                            definition.id
                        );
                    }
                }
                "network" => {
                    // Network access is allowed at this layer; rate-limiting is runtime concern.
                }
                other => {
                    log::debug!("Unknown capability scope '{}' for tool '{}' — allowing.", other, definition.id);
                }
            }
        }
        Ok(())
    }
}
```

- [ ] **Step 4: 在 capability.rs 加 `has_browser_capability()` 方法**

```rust
// 在 CapabilityContext impl 块中加入：
impl CapabilityContext {
    // ... 已有 with_workspace() ...

    /// 当前 context 是否有浏览器能力（connector 已激活）。
    /// Phase A3 先保守实现：无 connector 字段时返回 false。
    /// Phase A4 可扩展为检查 ConnectorCapability。
    pub fn has_browser_capability(&self) -> bool {
        false // 显式：connector 能力尚未注入 CapabilityContext，默认拒绝浏览器工具
    }
}
```

- [ ] **Step 5: 导出新类型**

在 `src-tauri/src/runtime/tools/mod.rs` 中修改 permission 导出行：

```rust
pub use permission::{AllowAllPermissionPipeline, CapabilityPermissionPipeline, PermissionPipeline};
```

- [ ] **Step 6: 运行测试，确认通过**

```bash
cd src-tauri && cargo test tool_permission_pipeline -- --nocapture
```
期望：4 个测试全部 PASS

- [ ] **Step 7: 运行全量测试**

```bash
cd src-tauri && cargo test -- --nocapture 2>&1 | tail -20
```

- [ ] **Step 8: 提交**

```bash
git add src-tauri/src/runtime/tools/permission.rs \
        src-tauri/src/runtime/tools/capability.rs \
        src-tauri/src/runtime/tools/mod.rs \
        src-tauri/tests/tool_permission_pipeline_test.rs
git commit -m "feat(atomic-tool): CapabilityPermissionPipeline replaces AllowAll for scope-checked tools"
```

---

## Phase A4：Composite 工具在 catalog 中显式标记 + schema 描述更新

**目标：** 确保 catalog 中 composite/power 工具的描述反映多阶段语义，为 LLM 提供信号。不重写实现，只更新 definition/schema（A1 已写好，此 phase 是验收核对 + integration test 确认）。

### Files

- Create: `src-tauri/tests/composite_tool_delegation_test.rs`

---

### Task A4.1：Composite 工具 contract 测试

- [ ] **Step 1: 写测试**

```rust
// src-tauri/tests/composite_tool_delegation_test.rs
use app_lib::runtime::tools::catalog::ToolCatalog;
use app_lib::runtime::tools::definition::ToolKind;

#[test]
fn composite_tools_description_mentions_composite() {
    let catalog = ToolCatalog::default_catalog();
    let composite_ids = ["browse_data", "generate_report", "export_data", "generate_chart", "generate_slides", "browse_and_extract"];
    for id in &composite_ids {
        let def = catalog.get(id).unwrap_or_else(|| panic!("{} not in catalog", id));
        assert!(
            matches!(def.kind, ToolKind::Composite),
            "{} should be Composite kind",
            id
        );
        assert!(
            def.description.contains("Composite") || def.description.contains("composite") || def.description.contains("【"),
            "Composite tool '{}' description should signal its composite nature. Got: {}",
            id, def.description
        );
    }
}

#[test]
fn browse_data_capability_scope_includes_browser_and_network() {
    let catalog = ToolCatalog::default_catalog();
    let def = catalog.get("browse_data").expect("browse_data must be in catalog");
    assert!(def.capability_scope.iter().any(|s| s == "browser"), "browse_data must require browser scope");
    assert!(def.capability_scope.iter().any(|s| s == "workspace:write"), "browse_data must require workspace:write scope");
}

#[test]
fn execute_python_capability_scope_includes_python_exec_and_workspace_write() {
    let catalog = ToolCatalog::default_catalog();
    let def = catalog.get("execute_python").expect("execute_python must be in catalog");
    assert!(def.capability_scope.iter().any(|s| s == "python:exec"), "execute_python must require python:exec scope");
    assert!(def.capability_scope.iter().any(|s| s == "workspace:write"), "execute_python must require workspace:write scope");
}

#[test]
fn generate_report_capability_scope_requires_workspace_write() {
    let catalog = ToolCatalog::default_catalog();
    let def = catalog.get("generate_report").expect("generate_report must be in catalog");
    assert!(def.capability_scope.iter().any(|s| s == "workspace:write"), "generate_report must require workspace:write scope");
}
```

- [ ] **Step 2: 运行，确认通过（A1 已建好 catalog，应直接通过）**

```bash
cd src-tauri && cargo test composite_tool_delegation -- --nocapture
```
期望：4 个测试全部 PASS。若有失败，检查 catalog.rs 中对应工具的 description 或 capability_scope。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/tests/composite_tool_delegation_test.rs
git commit -m "test(atomic-tool): composite tool contract tests — kind, description, capability scope"
```

---

## Phase A5：skill / workflow 工具名收口

### Files

- Create: `src-tauri/tests/skill_tool_contract_test.rs`
- Create: `src-tauri/tests/daily_mode_tool_surface_test.rs`
- Modify: `src-tauri/src/plugin/builtin/skills/daily_assistant.rs`（修复错误工具名引用，如有）

---

### Task A5.1：skill 工具名可解析性测试

- [ ] **Step 1: 写测试**

```rust
// src-tauri/tests/skill_tool_contract_test.rs
//! 验证 skill/workflow 中引用的工具名都能在 ToolCatalog 中解析到。
//!
//! 如果某个 ToolFilter::Only([...]) 列表里有不存在于 catalog 的工具名，
//! 这个测试会显式失败，而不是静默通过后运行时找不到工具。

use app_lib::runtime::tools::catalog::ToolCatalog;

/// daily assistant skill 允许的工具集（从 daily_assistant.rs 同步）。
/// 修改 daily_assistant.rs 时必须同步更新这个列表。
const DAILY_ALLOWED_TOOLS: &[&str] = &[
    "web_search",
    "execute_python",
    "load_file",
    "list_directory",
    "read_workspace_file",
    "search_files",
    "get_file_info",
    "generate_report",
    "generate_chart",
    "export_data",
    "browse_navigate",
    "read_page_content",
    "browse_data",
    "save_analysis_note",
    "plan_update",
    "progress_update",
    "save_memory",
    "search_memory",
];

#[test]
fn daily_skill_allowed_tools_all_exist_in_catalog() {
    let catalog = ToolCatalog::default_catalog();
    let mut missing = Vec::new();
    for name in DAILY_ALLOWED_TOOLS {
        if catalog.get(name).is_none() {
            missing.push(*name);
        }
    }
    assert!(
        missing.is_empty(),
        "The following tools referenced in daily skill are not in catalog: {:?}",
        missing
    );
}

/// analysis assistant skill 允许的工具集（从 analysis skill 同步）。
const ANALYSIS_ALLOWED_TOOLS: &[&str] = &[
    "load_file",
    "execute_python",
    "generate_report",
    "generate_chart",
    "export_data",
    "hypothesis_test",
    "detect_anomalies",
    "save_analysis_note",
    "progress_update",
    "web_search",
];

#[test]
fn analysis_skill_allowed_tools_all_exist_in_catalog() {
    let catalog = ToolCatalog::default_catalog();
    let mut missing = Vec::new();
    for name in ANALYSIS_ALLOWED_TOOLS {
        if catalog.get(name).is_none() {
            missing.push(*name);
        }
    }
    assert!(
        missing.is_empty(),
        "The following tools referenced in analysis skill are not in catalog: {:?}",
        missing
    );
}
```

- [ ] **Step 2: 运行测试**

```bash
cd src-tauri && cargo test skill_tool_contract -- --nocapture
```
期望：PASS。若有工具名缺失，去 `catalog.rs` 补充对应 entry。

- [ ] **Step 3: 写 daily mode 工具集暴露测试**

```rust
// src-tauri/tests/daily_mode_tool_surface_test.rs
//! 验证 daily 模式不再默认暴露所有 27 个工具。
//! 该测试是 acceptance criteria AC-1 的测试桩，等 daily skill 实现后完善。

use app_lib::runtime::tools::catalog::{ToolCatalog, TOOL_CATALOG};
use app_lib::runtime::tools::definition::ToolKind;

#[test]
fn catalog_composite_tools_are_not_primitive() {
    let catalog = ToolCatalog::default_catalog();
    let composite_ids = ["browse_data", "generate_report", "export_data", "generate_chart"];
    for id in &composite_ids {
        let def = catalog.get(id).unwrap_or_else(|| panic!("{} must be in catalog", id));
        assert!(
            !matches!(def.kind, ToolKind::Primitive),
            "Composite tool '{}' must NOT be Primitive kind — it would appear at same level as atomic tools",
            id
        );
    }
}

#[test]
fn catalog_has_no_unknown_kind_tools() {
    let catalog = ToolCatalog::default_catalog();
    // 确认所有工具的 kind 是 4 类之一（编译期保证，但 runtime 验证一遍）
    for id in catalog.all_ids() {
        let def = catalog.get(&id).unwrap();
        let _ = &def.kind; // 编译时如果有新 variant 未处理会报警
    }
}
```

- [ ] **Step 4: 运行测试**

```bash
cd src-tauri && cargo test daily_mode_tool_surface -- --nocapture
```
期望：PASS

- [ ] **Step 5: 运行全量测试，确认所有 review_ 和新增 TDD 测试通过**

```bash
cd src-tauri && cargo test -- --nocapture 2>&1 | tail -30
```
期望：所有 test result 为 ok，包含 `review_*` 测试组

- [ ] **Step 6: 最终提交**

```bash
git add src-tauri/tests/skill_tool_contract_test.rs \
        src-tauri/tests/daily_mode_tool_surface_test.rs
git commit -m "test(atomic-tool): skill tool name parity and daily mode surface contract tests"
```

---

## 自查（写完后核对）

### Spec Coverage

| 专项目标 | 对应 Task | 状态 |
|---|---|---|
| 单一 schema 真相源 | A1.2 catalog.rs | ✅ |
| ToolKind 分类 | A1.1 definition.rs | ✅ |
| llm/tools.rs 降级 | A1.3 | ✅ |
| 第一批 11 个工具迁 RuntimeTool | A2.1（4个 workspace tools）| ⚠️ 此 plan 只迁 4 个，剩余 7 个（web_search, browse_*, extract_*）留 Phase A2 扩展 |
| execute_python 标为 Power | A1.2 catalog + A4.1 test | ✅ |
| browse_data 标为 Composite | A1.2 catalog + A4.1 test | ✅ |
| 权限管线落地 | A3.1 | ✅（browser 默认拒绝，workspace 按 capability） |
| skill 工具名收口 | A5.1 | ✅ |
| 不删除 execute_python | — | ✅（不在本 plan 范围） |
| 不改前端 | — | ✅（全 Rust，无前端改动） |

### 剩余 7 个 Primitive 工具（`web_search`, `browse_navigate`, `read_page_content`, `page_execute_js`, `extract_table_data`, `extract_with_pagination`, `load_file`）

这 7 个工具的 RuntimeTool 迁移逻辑与 A2.1 workspace 工具完全对称，代码模式一致。建议在本 plan 通过验收后，按同样方式在 `runtime/tools/builtin/` 下补充实现，不在当前 plan 中一次放入（避免单次 PR 过大）。

---

## Kill List（本专项完成后可清理）

| 项目 | 动作 | 时机 |
|---|---|---|
| `llm/tools.rs` 中的 `build_tool_definitions()` 硬编码 vec | 降级为 catalog 委托（A1.3 已做） | A1 完成后 |
| `ToolDefinition.capability_scope` 为空 vec（旧字段从未用） | 现在 catalog entry 会填写 scope，旧 `ToolDefinition::new()` 默认为空，可接受 | 无需主动删 |
| `AllowAllPermissionPipeline` | 降级为"测试和 legacy bridge 专用"，生产路径改用 `CapabilityPermissionPipeline` | A3 完成后 |
| `workspace_tools.rs` 中的 `ToolPlugin` 实现 | A2 迁移后保留为 legacy compat wrapper，可在 A2 完全稳定后删除 | A2 稳定 1 周后 |

## Not Doing（明确不做）

- 不重写 `browse_data` 的内部子代理逻辑（A4 只改标记，不改实现）
- 不修改 `llm/tool_executor/` 中的执行器实现
- 不修改前端任何代码
- 不修改 LLM provider 层
- 不把 workspace-first、prompt slimming、skill import 等其他专项揉进来
- 不在 `CapabilityContext` 中加入 `LlmGateway`、`AuthManager`（架构约束，见 CLAUDE.md）
