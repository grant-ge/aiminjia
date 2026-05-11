//! Persona storage — user-defined roles with identity, expertise, and memory hints.
//!
//! # DEPRECATED (2026-05-10)
//!
//! Persona 系统已进入退役流程。新方向是数字员工（Employee, `runtime/employee/`）一统：
//! Employee 已覆盖 Persona 全部能力（identity / expertise via `system_prompt_extra`）
//! 加上 cron / tool_whitelist / default_skill_id / resource_config / lifecycle 等。
//!
//! PR-5 会把 `AgendaItem.organizer_persona_id` 迁到 `organizer_employee_id`。
//! 在 agenda 切换完成前，本模块仍被 agenda dispatcher（`transport/tauri_commands/chat.rs`）
//! 依赖，所以不加 `#[deprecated]` attribute；新代码请直接使用 `runtime::employee`。
//!
//! Directory layout:
//! ```text
//! {base_dir}/personas/
//! ├── index.json          # { order: [id...], active: "default" }
//! ├── default.json        # Builtin: 通用工作助手
//! ├── hr-expert.json      # Builtin: HR 专家
//! ├── finance-analyst.json
//! ├── sales-manager.json
//! ├── ops-specialist.json
//! ├── legal-advisor.json
//! ├── data-analyst.json
//! ├── project-manager.json
//! └── {uuid}.json         # User-defined personas
//! ```

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::io::atomic_write_json;

/// Persona definition — role identity + expertise + memory hints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Persona {
    pub id: String,
    pub version: u32,
    pub builtin: bool,
    pub name: String,
    pub icon: String,
    pub description: String,
    #[serde(default)]
    pub name_en: String,
    #[serde(default)]
    pub description_en: String,
    /// Identity injected into system prompt (role setting).
    pub identity: String,
    /// Areas of expertise (for display).
    pub expertise: Vec<String>,
    /// Memory hints override (replaces daily.md memory whitelist).
    pub memory_hints: Vec<String>,
    /// Linked skill categories (WelcomeScreen expands these groups).
    pub linked_categories: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Persona index — order + active persona.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersonaIndex {
    order: Vec<String>,
    active: String,
}

/// Persona summary for list API (without full content).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonaSummary {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub description: String,
    pub builtin: bool,
    #[serde(default)]
    pub name_en: String,
    #[serde(default)]
    pub description_en: String,
}

/// Persona export format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersonaExport {
    format: String,
    version: u32,
    exported_at: String,
    persona: PersonaExportData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersonaExportData {
    name: String,
    icon: String,
    description: String,
    identity: String,
    expertise: Vec<String>,
    memory_hints: Vec<String>,
    linked_categories: Vec<String>,
}

// ═══════════════════════════════════════════════════════════════════════
// Initialization
// ═══════════════════════════════════════════════════════════════════════

/// Ensure personas directory exists and seed builtin personas.
pub fn ensure_personas_dir(base_dir: &Path) -> Result<()> {
    let personas_dir = base_dir.join("personas");
    fs::create_dir_all(&personas_dir)?;

    let builtins = builtin_personas();

    // Seed builtin personas if they don't exist
    for persona in &builtins {
        let path = personas_dir.join(format!("{}.json", persona.id));
        if !path.exists() {
            atomic_write_json(&path, persona)?;
        }
    }

    // Initialize index if it doesn't exist
    let index_path = personas_dir.join("index.json");
    if !index_path.exists() {
        let index = PersonaIndex {
            order: builtins.iter().map(|p| p.id.clone()).collect(),
            active: "default".to_string(),
        };
        atomic_write_json(&index_path, &index)?;
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// CRUD Operations
// ═══════════════════════════════════════════════════════════════════════

/// List all personas (summaries only).
pub fn list_personas(base_dir: &Path) -> Result<Vec<PersonaSummary>> {
    let personas_dir = base_dir.join("personas");
    let index = read_index(base_dir)?;

    let mut summaries = Vec::new();
    for id in &index.order {
        let path = personas_dir.join(format!("{}.json", id));
        if let Ok(persona) = read_persona_file(&path) {
            summaries.push(PersonaSummary {
                id: persona.id,
                name: persona.name,
                icon: persona.icon,
                description: persona.description,
                builtin: persona.builtin,
                name_en: persona.name_en.clone(),
                description_en: persona.description_en.clone(),
            });
        }
    }

    Ok(summaries)
}

/// Get a persona by ID.
pub fn get_persona(base_dir: &Path, id: &str) -> Result<Persona> {
    let path = base_dir.join("personas").join(format!("{}.json", id));
    read_persona_file(&path)
}

/// Save a persona (create or update).
pub fn save_persona(base_dir: &Path, persona: &Persona) -> Result<()> {
    let personas_dir = base_dir.join("personas");
    let path = personas_dir.join(format!("{}.json", persona.id));

    // Update timestamp
    let mut p = persona.clone();
    p.updated_at = chrono::Utc::now().to_rfc3339();
    if !path.exists() {
        p.created_at = p.updated_at.clone();
    }

    atomic_write_json(&path, &p)?;

    // Add to index if new
    let mut index = read_index(base_dir)?;
    if !index.order.contains(&persona.id) {
        index.order.push(persona.id.clone());
        write_index(base_dir, &index)?;
    }

    Ok(())
}

/// Delete a persona by ID.
pub fn delete_persona(base_dir: &Path, id: &str) -> Result<()> {
    // Prevent deleting builtin personas
    let persona = get_persona(base_dir, id)?;
    if persona.builtin {
        anyhow::bail!("Cannot delete builtin persona: {}", id);
    }

    // Remove file
    let path = base_dir.join("personas").join(format!("{}.json", id));
    fs::remove_file(&path)?;

    // Remove from index
    let mut index = read_index(base_dir)?;
    index.order.retain(|i| i != id);

    // If active persona was deleted, reset to default
    if index.active == id {
        index.active = "default".to_string();
    }

    write_index(base_dir, &index)?;

    Ok(())
}

/// Get active persona ID.
pub fn get_active_persona_id(base_dir: &Path) -> Result<String> {
    let index = read_index(base_dir)?;
    Ok(index.active)
}

/// Set active persona.
pub fn set_active_persona(base_dir: &Path, id: &str) -> Result<()> {
    // Verify persona exists
    get_persona(base_dir, id)?;

    let mut index = read_index(base_dir)?;
    index.active = id.to_string();
    write_index(base_dir, &index)?;

    Ok(())
}

/// Export a persona to JSON.
pub fn export_persona(base_dir: &Path, id: &str) -> Result<String> {
    let persona = get_persona(base_dir, id)?;

    let export = PersonaExport {
        format: "aijia-persona".to_string(),
        version: 1,
        exported_at: chrono::Utc::now().to_rfc3339(),
        persona: PersonaExportData {
            name: persona.name,
            icon: persona.icon,
            description: persona.description,
            identity: persona.identity,
            expertise: persona.expertise,
            memory_hints: persona.memory_hints,
            linked_categories: persona.linked_categories,
        },
    };

    serde_json::to_string_pretty(&export).context("Failed to serialize persona export")
}

/// Import a persona from JSON.
/// Returns the new persona ID.
pub fn import_persona(base_dir: &Path, json: &str) -> Result<String> {
    let export: PersonaExport =
        serde_json::from_str(json).context("Invalid persona export format")?;

    if export.format != "aijia-persona" {
        anyhow::bail!("Invalid export format: {}", export.format);
    }

    // Generate new UUID
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let persona = Persona {
        id: id.clone(),
        version: 1,
        builtin: false,
        name: export.persona.name,
        icon: export.persona.icon,
        description: export.persona.description,
        name_en: String::new(),
        description_en: String::new(),
        identity: export.persona.identity,
        expertise: export.persona.expertise,
        memory_hints: export.persona.memory_hints,
        linked_categories: export.persona.linked_categories,
        created_at: now.clone(),
        updated_at: now,
    };

    save_persona(base_dir, &persona)?;

    Ok(id)
}

// ═══════════════════════════════════════════════════════════════════════
// Builtin Personas
// ═══════════════════════════════════════════════════════════════════════

fn builtin_personas() -> Vec<Persona> {
    let now = chrono::Utc::now().to_rfc3339();

    vec![
        Persona {
            id: "default".to_string(),
            version: 1,
            builtin: true,
            name: "通用工作助手".to_string(),
            icon: "🏠".to_string(),
            description: "适合日常工作的通用助手，可处理数据分析、文档生成、翻译搜索等各类任务".to_string(),
            name_en: "General Assistant".to_string(),
            description_en: "General-purpose work assistant for data analysis, document generation, translation, and search".to_string(),
            identity: "你是用户的通用工作助手，可处理各类日常工作任务。".to_string(),
            expertise: vec![
                "数据处理与分析".to_string(),
                "文档编写与生成".to_string(),
                "文件管理".to_string(),
                "翻译与搜索".to_string(),
            ],
            memory_hints: vec![
                "用户身份：所在行业、公司规模、工作领域、常用工具".to_string(),
                "用户偏好：展示偏好（图表类型、导出格式）和分析偏好（关注维度、方法论）".to_string(),
                "已确认方法论：用户确认过的分析框架、统计方法、业务规则".to_string(),
                "数据源硬特征：反复出现的数据质量问题、数据固有结构特征".to_string(),
                "已验证结论：用户看过并认可的分析发现，不是 AI 单方面推断".to_string(),
            ],
            linked_categories: vec!["general".to_string()],
            created_at: now.clone(),
            updated_at: now.clone(),
        },
        Persona {
            id: "hr-expert".to_string(),
            version: 1,
            builtin: true,
            name: "HR 专家".to_string(),
            icon: "👔".to_string(),
            description: "资深 HR 顾问，擅长薪酬分析、组织设计、人才盘点、绩效体系等".to_string(),
            name_en: "HR Expert".to_string(),
            description_en: "Senior HR consultant specializing in compensation analysis, org design, talent review, and performance systems".to_string(),
            identity: "你是一位资深的 HR 专家和组织咨询顾问，擅长薪酬分析、岗位评估、组织设计、人才盘点、绩效体系设计等。".to_string(),
            expertise: vec![
                "薪酬公平性分析".to_string(),
                "组织诊断与设计".to_string(),
                "人才盘点与继任计划".to_string(),
                "绩效体系设计".to_string(),
                "劳动法规咨询".to_string(),
            ],
            memory_hints: vec![
                "企业身份：行业、规模、组织架构、薪酬结构、财年/调薪周期".to_string(),
                "用户偏好：展示偏好（图表类型、导出格式）和分析偏好（对标分位数、关注维度）".to_string(),
                "已确认方法论：用户确认过的岗位归一化方案、职级框架、统计方法".to_string(),
                "数据源硬特征：反复出现的数据质量问题（长期高空值率）、数据固有结构特征".to_string(),
                "已验证结论：用户看过并认可的分析发现，不是 AI 单方面推断".to_string(),
            ],
            linked_categories: vec!["hr".to_string()],
            created_at: now.clone(),
            updated_at: now.clone(),
        },
        Persona {
            id: "finance-analyst".to_string(),
            version: 1,
            builtin: true,
            name: "财务分析师".to_string(),
            icon: "💹".to_string(),
            description: "专业财务分析师，擅长财务报表分析、预算管理、成本控制、投资分析".to_string(),
            name_en: "Finance Analyst".to_string(),
            description_en: "Professional financial analyst specializing in financial statements, budgeting, cost control, and investment analysis".to_string(),
            identity: "你是一位专业的财务分析师，擅长财务报表分析、预算管理、成本控制、投资分析等。".to_string(),
            expertise: vec![
                "财务报表分析".to_string(),
                "预算编制与管理".to_string(),
                "成本控制与优化".to_string(),
                "投资分析与决策".to_string(),
            ],
            memory_hints: vec![
                "企业身份：行业、规模、财务周期、会计准则".to_string(),
                "用户偏好：报表格式、分析维度、关注指标".to_string(),
                "已确认方法论：财务分析框架、成本分摊方法、预算编制规则".to_string(),
                "数据源硬特征：财务系统特征、数据更新频率".to_string(),
                "已验证结论：用户认可的财务分析发现".to_string(),
            ],
            linked_categories: vec!["finance".to_string()],
            created_at: now.clone(),
            updated_at: now.clone(),
        },
        Persona {
            id: "sales-manager".to_string(),
            version: 1,
            builtin: true,
            name: "销售经理".to_string(),
            icon: "📈".to_string(),
            description: "资深销售管理者，擅长销售数据分析、客户管理、销售预测、团队管理".to_string(),
            name_en: "Sales Manager".to_string(),
            description_en: "Senior sales manager specializing in sales analytics, customer management, sales forecasting, and team management".to_string(),
            identity: "你是一位资深的销售经理，擅长销售数据分析、客户管理、销售预测、团队管理等。".to_string(),
            expertise: vec![
                "销售数据分析".to_string(),
                "客户细分与管理".to_string(),
                "销售预测与目标设定".to_string(),
                "销售团队管理".to_string(),
            ],
            memory_hints: vec![
                "企业身份：行业、产品类型、销售模式、客户类型".to_string(),
                "用户偏好：关注指标（转化率、客单价等）、分析维度".to_string(),
                "已确认方法论：客户分类标准、销售漏斗定义、预测模型".to_string(),
                "数据源硬特征：CRM 系统特征、数据更新频率".to_string(),
                "已验证结论：用户认可的销售分析发现".to_string(),
            ],
            linked_categories: vec!["sales".to_string()],
            created_at: now.clone(),
            updated_at: now.clone(),
        },
        Persona {
            id: "ops-specialist".to_string(),
            version: 1,
            builtin: true,
            name: "运营专家".to_string(),
            icon: "🛒".to_string(),
            description: "资深运营专家，擅长用户运营与增长策划、数据分析、增长策略".to_string(),
            name_en: "Operations Specialist".to_string(),
            description_en: "Senior operations expert specializing in user growth, campaign planning, data analysis, and growth strategy".to_string(),
            identity: "你是一位资深的运营专家，擅长用户运营与增长策划、数据分析、增长策略等。".to_string(),
            expertise: vec![
                "用户运营与增长".to_string(),
                "活动策划与执行".to_string(),
                "运营数据分析".to_string(),
                "增长策略设计".to_string(),
            ],
            memory_hints: vec![
                "企业身份：行业、产品类型、用户规模、运营模式".to_string(),
                "用户偏好：关注指标（DAU、留存率等）、分析维度".to_string(),
                "已确认方法论：用户分层标准、活动效果评估方法".to_string(),
                "数据源硬特征：数据埋点特征、数据更新频率".to_string(),
                "已验证结论：用户认可的运营分析发现".to_string(),
            ],
            linked_categories: vec!["ops".to_string()],
            created_at: now.clone(),
            updated_at: now.clone(),
        },
        Persona {
            id: "legal-advisor".to_string(),
            version: 1,
            builtin: true,
            name: "法务顾问".to_string(),
            icon: "⚖️".to_string(),
            description: "专业法务顾问，擅长合同审查、法律风险评估、合规咨询".to_string(),
            name_en: "Legal Advisor".to_string(),
            description_en: "Professional legal advisor specializing in contract review, legal risk assessment, and compliance consulting".to_string(),
            identity: "你是一位专业的法务顾问，擅长合同审查、法律风险评估、合规咨询等。".to_string(),
            expertise: vec![
                "合同审查与起草".to_string(),
                "法律风险评估".to_string(),
                "合规咨询".to_string(),
                "劳动法规咨询".to_string(),
            ],
            memory_hints: vec![
                "企业身份：行业、业务类型、常见法律事务".to_string(),
                "用户偏好：关注风险类型、审查重点".to_string(),
                "已确认方法论：合同审查标准、风险评估框架".to_string(),
                "数据源硬特征：合同模板特征、常见条款".to_string(),
                "已验证结论：用户认可的法律分析发现".to_string(),
            ],
            linked_categories: vec!["legal".to_string()],
            created_at: now.clone(),
            updated_at: now.clone(),
        },
        Persona {
            id: "data-analyst".to_string(),
            version: 1,
            builtin: true,
            name: "数据分析师".to_string(),
            icon: "📊".to_string(),
            description: "专业数据分析师，擅长数据清洗、统计分析、可视化、机器学习".to_string(),
            name_en: "Data Analyst".to_string(),
            description_en: "Professional data analyst specializing in data cleaning, statistical analysis, visualization, and machine learning".to_string(),
            identity: "你是一位专业的数据分析师，擅长数据清洗、统计分析、数据可视化、机器学习等。".to_string(),
            expertise: vec![
                "数据清洗与处理".to_string(),
                "统计分析".to_string(),
                "数据可视化".to_string(),
                "机器学习建模".to_string(),
            ],
            memory_hints: vec![
                "用户身份：所在行业、分析领域、常用工具".to_string(),
                "用户偏好：可视化偏好、统计方法偏好".to_string(),
                "已确认方法论：数据清洗规则、统计方法、建模方法".to_string(),
                "数据源硬特征：数据质量问题、数据结构特征".to_string(),
                "已验证结论：用户认可的数据分析发现".to_string(),
            ],
            linked_categories: vec!["general".to_string()],
            created_at: now.clone(),
            updated_at: now.clone(),
        },
        Persona {
            id: "project-manager".to_string(),
            version: 1,
            builtin: true,
            name: "项目经理".to_string(),
            icon: "🎯".to_string(),
            description: "资深项目经理，擅长项目规划、进度管理、风险控制、团队协作".to_string(),
            name_en: "Project Manager".to_string(),
            description_en: "Senior project manager specializing in project planning, schedule management, risk control, and team collaboration".to_string(),
            identity: "你是一位资深的项目经理，擅长项目规划、进度管理、风险控制、团队协作等。".to_string(),
            expertise: vec![
                "项目规划与分解".to_string(),
                "进度管理与跟踪".to_string(),
                "风险识别与控制".to_string(),
                "团队协作与沟通".to_string(),
            ],
            memory_hints: vec![
                "企业身份：行业、项目类型、团队规模".to_string(),
                "用户偏好：项目管理方法（敏捷/瀑布）、关注指标".to_string(),
                "已确认方法论：项目分解方法、风险评估框架".to_string(),
                "数据源硬特征：项目管理工具特征、数据更新频率".to_string(),
                "已验证结论：用户认可的项目分析发现".to_string(),
            ],
            linked_categories: vec!["general".to_string()],
            created_at: now.clone(),
            updated_at: now,
        },
    ]
}

// ═══════════════════════════════════════════════════════════════════════
// Internal Helpers
// ═══════════════════════════════════════════════════��═══════════════════

fn read_index(base_dir: &Path) -> Result<PersonaIndex> {
    let path = base_dir.join("personas").join("index.json");
    let content = fs::read_to_string(&path).context("Failed to read persona index")?;
    serde_json::from_str(&content).context("Failed to parse persona index")
}

fn write_index(base_dir: &Path, index: &PersonaIndex) -> Result<()> {
    let path = base_dir.join("personas").join("index.json");
    atomic_write_json(&path, index)?;
    Ok(())
}

fn read_persona_file(path: &Path) -> Result<Persona> {
    let content = fs::read_to_string(path).context("Failed to read persona file")?;
    serde_json::from_str(&content).context("Failed to parse persona file")
}
