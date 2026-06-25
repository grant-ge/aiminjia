use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::commands::skill_management::{
    install_marketplace_skill_headless_with_auth, install_marketplace_skill_with_auth,
    list_marketplace_skills_with_auth, MarketplaceSkillItem,
};
use crate::plugin::skill::enablement::{SkillEnablementState, SkillEnablementStore};
use crate::plugin::skill::registry::SkillRegistry;
use crate::plugin::skill::types::{DiskSkill, SkillSource};
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::{ToolDefinition, ToolKind};
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

const SEARCH_TOOL: &str = "SkillMarketSearch";
const INSTALL_TOOL: &str = "SkillMarketInstall";
const DEFAULT_MAX_RESULTS: usize = 3;
const MAX_RESULTS_LIMIT: usize = 5;
const MIN_MATCH_SCORE: i64 = 10;
const COMPETITIVE_SCORE_RATIO_PERCENT: i64 = 30;
const DISABLED_INSTALLED_BLOCK_SCORE: i64 = 70;
const INSTALLED_DISABLED_MESSAGE: &str =
    "该技能已安装但当前用户已关闭。不要重新安装，也不要绕过关闭状态使用该技能。";

#[derive(Clone)]
pub struct SkillMarketSearchRuntimeTool {
    auth_manager: Arc<crate::auth::AuthManager>,
    skill_registry: Arc<Mutex<SkillRegistry>>,
    enablement_store: Option<Arc<SkillEnablementStore>>,
}

impl SkillMarketSearchRuntimeTool {
    pub fn new(
        auth_manager: Arc<crate::auth::AuthManager>,
        skill_registry: Arc<Mutex<SkillRegistry>>,
        enablement_store: Option<Arc<SkillEnablementStore>>,
    ) -> Self {
        Self {
            auth_manager,
            skill_registry,
            enablement_store,
        }
    }
}

#[derive(Clone)]
pub struct SkillMarketInstallRuntimeTool {
    install_target: SkillMarketInstallTarget,
    auth_manager: Arc<crate::auth::AuthManager>,
    skill_registry: Arc<Mutex<SkillRegistry>>,
    enablement_store: Option<Arc<SkillEnablementStore>>,
}

#[derive(Clone)]
enum SkillMarketInstallTarget {
    Tauri(AppHandle),
    Headless(HeadlessSkillMarketInstallRoots),
}

#[derive(Clone)]
pub struct HeadlessSkillMarketInstallRoots {
    pub user_skills_dir: PathBuf,
    pub global_skills_dir: PathBuf,
    pub tmp_dir: PathBuf,
}

impl SkillMarketInstallRuntimeTool {
    pub fn new(
        app: AppHandle,
        auth_manager: Arc<crate::auth::AuthManager>,
        skill_registry: Arc<Mutex<SkillRegistry>>,
        enablement_store: Option<Arc<SkillEnablementStore>>,
    ) -> Self {
        Self {
            install_target: SkillMarketInstallTarget::Tauri(app),
            auth_manager,
            skill_registry,
            enablement_store,
        }
    }

    pub fn new_headless(
        auth_manager: Arc<crate::auth::AuthManager>,
        skill_registry: Arc<Mutex<SkillRegistry>>,
        enablement_store: Option<Arc<SkillEnablementStore>>,
        roots: HeadlessSkillMarketInstallRoots,
    ) -> Self {
        Self {
            install_target: SkillMarketInstallTarget::Headless(roots),
            auth_manager,
            skill_registry,
            enablement_store,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillMarketCandidate {
    package_id: i64,
    plugin_id: String,
    name: String,
    description_snippet: String,
    category: String,
    score: i64,
    confidence: &'static str,
    reasons: Vec<&'static str>,
}

struct RankedCandidate {
    item: MarketplaceSkillItem,
    score: i64,
    reasons: Vec<&'static str>,
}

struct LocalSkillMatch {
    skill_id: String,
}

#[async_trait]
impl RuntimeTool for SkillMarketSearchRuntimeTool {
    fn id(&self) -> &str {
        SEARCH_TOOL
    }

    fn default_read_only(&self) -> bool {
        true
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
        ToolDefinition::new(
            SEARCH_TOOL,
            "根据用户原始任务搜索企业技能市场，只返回少量候选技能。调用本工具前必须先调用 Skill({skill_id:\"find-skills\"}) 加载发现技能指令；用于当前已启用 skill catalog 没有明显覆盖专项任务时。普通公开网页、简单事实查询、闲聊或已启用技能明确覆盖的任务不要调用。\n\n搜索无候选、候选置信度低、技能已关闭或市场请求失败，不代表用户任务结束。记录技能发现结果后，继续完成其它可执行交付物；只有用户必须在多个候选中选择、或缺少安装授权/关键目标时才澄清。若恰好一个候选与任务高度匹配，先安装再 RefreshSkills，并按新技能继续执行。不要用本工具替代本地 SKILL.md 发现；本地技能用文件搜索和 Read。",
        )
        .with_kind(ToolKind::Support)
        .with_read_only(true)
        .with_capability_scope(["network"])
        .with_max_result_size_chars(8_000)
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let task = input
            .get("task")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required field: task".into()))?;
        let hints = input
            .get("capabilityHints")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .take(5)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let max_results = input
            .get("maxResults")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or(DEFAULT_MAX_RESULTS)
            .clamp(1, MAX_RESULTS_LIMIT);

        if !find_skills_loaded_for_current_conversation(&ctx) {
            let payload = json!({
                "status": "requires_find_skills",
                "task": task,
                "message": "调用 SkillMarketSearch 前必须先调用 Skill(skill_id=\"find-skills\") 加载发现技能指令；加载后再根据 find-skills 的规则搜索、询问或安装。不要直接安装技能。",
                "candidates": [],
            });
            return Ok(ToolResult::new(
                SEARCH_TOOL,
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string()),
                Some(payload),
            ));
        }

        let installed_skills = self.installed_skills()?;
        let installed_ids = installed_skills
            .iter()
            .map(|skill| skill.id.clone())
            .collect::<HashSet<_>>();
        let enablement = self.enablement_state();

        if let Some(disabled) =
            best_disabled_local_skill_match(&installed_skills, task, &hints, &enablement)
        {
            let payload = json!({
                "status": "installed_disabled",
                "task": task,
                "installedSkillId": disabled.skill_id,
                "skillId": disabled.skill_id,
                "message": INSTALLED_DISABLED_MESSAGE,
                "candidates": [],
            });
            return Ok(ToolResult::new(
                SEARCH_TOOL,
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string()),
                Some(payload),
            ));
        }

        let response =
            list_marketplace_skills_with_auth(self.auth_manager.clone(), 1, 100, None, None)
                .await
                .map_err(ToolError::ExecutionFailed)?;
        let ranked = rank_marketplace_candidates(&response.items, task, &hints);

        if let Some(top_disabled) =
            best_disabled_installed_match(&ranked, &installed_ids, &enablement)
        {
            let payload = json!({
                "status": "installed_disabled",
                "task": task,
                "installedSkillId": top_disabled.item.plugin_id,
                "skillId": top_disabled.item.plugin_id,
                "message": INSTALLED_DISABLED_MESSAGE,
                "candidates": [],
            });
            return Ok(ToolResult::new(
                SEARCH_TOOL,
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string()),
                Some(payload),
            ));
        }

        if let Some(top_installed) = best_installed_match(&ranked, &installed_ids, &enablement) {
            let payload = json!({
                "status": "already_installed",
                "task": task,
                "installedSkillId": top_installed.item.plugin_id,
                "candidates": [],
            });
            return Ok(ToolResult::new(
                SEARCH_TOOL,
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string()),
                Some(payload),
            ));
        }

        let candidates = competitive_uninstalled_candidates(ranked, &installed_ids, max_results);

        let (status, message) = search_status_for_candidate_count(candidates.len());
        let payload = json!({
            "status": status,
            "task": task,
            "candidates": candidates,
            "message": message,
            "truncated": response.total > response.items.len() as i64,
        });
        Ok(ToolResult::new(
            SEARCH_TOOL,
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string()),
            Some(payload),
        ))
    }
}

#[async_trait]
impl RuntimeTool for SkillMarketInstallRuntimeTool {
    fn id(&self) -> &str {
        INSTALL_TOOL
    }

    fn default_destructive(&self) -> bool {
        true
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
        ToolDefinition::new(
            INSTALL_TOOL,
            "安装 SkillMarketSearch 返回的受信任市场技能。调用前必须确认 packageId 与 pluginId 来自本轮搜索候选；本工具不是 GitHub/URL/本地目录安装器，不能用来把未经审查的外部仓库、压缩包或用户代码装入 `~/skills`、`.agents/skills`、工作区 `skills/` 等自动加载目录。如果同名技能已经安装，本工具只返回 alreadyInstalled；如果该技能已关闭，会提示不要重新安装或绕过关闭状态。安装成功后调用 RefreshSkills；如果任务需要立即使用该技能，随后调用 Skill(skill_id=已安装 pluginId) 读取技能说明再执行。安装失败、alreadyInstalled 或 disabled 不是最终交付，继续处理用户任务中其它可执行部分，并把技能状态写入最终结果或阻塞说明。",
        )
        .with_kind(ToolKind::Support)
        .with_destructive(true)
        .with_capability_scope(["network"])
        .with_default_timeout_secs(180)
        .with_max_result_size_chars(4_000)
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let package_id = input
            .get("packageId")
            .or_else(|| input.get("package_id"))
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                ToolError::ExecutionFailed("Missing required field: packageId".into())
            })?;
        let plugin_id = input
            .get("pluginId")
            .or_else(|| input.get("plugin_id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required field: pluginId".into()))?
            .to_string();

        if let Some(enabled) = self.installed_enabled_state(&plugin_id)? {
            let mut payload = json!({
                "installed": false,
                "alreadyInstalled": true,
                "enabled": enabled,
                "pluginId": plugin_id,
                "skillId": plugin_id,
                "refreshed": false,
            });
            if !enabled {
                payload["blockedReason"] = json!("disabled_by_user");
                payload["message"] = json!(INSTALLED_DISABLED_MESSAGE);
            }
            return Ok(ToolResult::new(
                INSTALL_TOOL,
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string()),
                Some(payload),
            ));
        }

        match &self.install_target {
            SkillMarketInstallTarget::Tauri(app) => {
                let _message = install_marketplace_skill_with_auth(
                    app.clone(),
                    self.auth_manager.clone(),
                    package_id,
                    plugin_id.clone(),
                )
                .await
                .map_err(ToolError::ExecutionFailed)?;
            }
            SkillMarketInstallTarget::Headless(roots) => {
                let _message = install_marketplace_skill_headless_with_auth(
                    self.auth_manager.clone(),
                    package_id,
                    plugin_id.clone(),
                    roots.user_skills_dir.clone(),
                    roots.tmp_dir.clone(),
                )
                .await
                .map_err(ToolError::ExecutionFailed)?;
                if let Some(store) = &self.enablement_store {
                    store
                        .clear_override(&plugin_id)
                        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                }
                self.refresh_headless_registry(roots)?;
            }
        }

        let payload = json!({
            "installed": true,
            "pluginId": plugin_id,
            "skillId": plugin_id,
            "refreshed": true,
            "message": format!("Installed '{}'", plugin_id),
            "nextAction": format!("Call Skill with skill_id={} before using the new capability.", plugin_id),
        });
        Ok(ToolResult::new(
            INSTALL_TOOL,
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string()),
            Some(payload),
        ))
    }
}

impl SkillMarketSearchRuntimeTool {
    fn enablement_state(&self) -> SkillEnablementState {
        self.enablement_store
            .as_ref()
            .map(|store| store.load_or_default())
            .unwrap_or_default()
    }

    fn installed_skills(&self) -> Result<Vec<DiskSkill>, ToolError> {
        let reg = self
            .skill_registry
            .lock()
            .map_err(|e| ToolError::ExecutionFailed(format!("Registry lock failed: {}", e)))?;
        Ok(reg
            .skill_ids()
            .into_iter()
            .filter_map(|id| reg.get(&id).cloned())
            .collect())
    }
}

impl SkillMarketInstallRuntimeTool {
    fn enablement_state(&self) -> SkillEnablementState {
        self.enablement_store
            .as_ref()
            .map(|store| store.load_or_default())
            .unwrap_or_default()
    }

    fn installed_enabled_state(&self, plugin_id: &str) -> Result<Option<bool>, ToolError> {
        let enablement = self.enablement_state();
        let reg = self
            .skill_registry
            .lock()
            .map_err(|e| ToolError::ExecutionFailed(format!("Registry lock failed: {}", e)))?;
        Ok(reg.get(plugin_id).map(|_| enablement.is_enabled(plugin_id)))
    }

    fn refresh_headless_registry(
        &self,
        roots: &HeadlessSkillMarketInstallRoots,
    ) -> Result<(), ToolError> {
        let loaded = crate::plugin::skill::loader::load_skill_roots_tagged(&[
            (roots.user_skills_dir.clone(), SkillSource::User),
            (roots.global_skills_dir.clone(), SkillSource::Global),
        ])
        .map_err(|e| ToolError::ExecutionFailed(format!("load_skill_roots failed: {}", e)))?;
        self.skill_registry
            .lock()
            .map_err(|e| ToolError::ExecutionFailed(format!("Registry lock failed: {}", e)))?
            .replace_all(loaded.into_values().collect());
        Ok(())
    }
}

fn to_search_candidate(candidate: RankedCandidate) -> SkillMarketCandidate {
    let description_snippet = first_chars(&candidate.item.description, 120);
    SkillMarketCandidate {
        package_id: candidate.item.id,
        plugin_id: candidate.item.plugin_id.clone(),
        name: candidate.item.name.clone(),
        description_snippet,
        category: candidate.item.category.clone(),
        score: candidate.score,
        confidence: confidence(candidate.score),
        reasons: candidate.reasons,
    }
}

fn rank_marketplace_candidates(
    items: &[MarketplaceSkillItem],
    task: &str,
    capability_hints: &[String],
) -> Vec<RankedCandidate> {
    let terms = search_terms(task, capability_hints);
    let mut ranked = items
        .iter()
        .filter_map(|item| {
            let (score, reasons) = score_item(item, &terms, capability_hints);
            if score <= 0 {
                None
            } else {
                Some(RankedCandidate {
                    item: item.clone(),
                    score,
                    reasons,
                })
            }
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.item.featured.cmp(&a.item.featured))
            .then_with(|| b.item.downloads.cmp(&a.item.downloads))
            .then_with(|| a.item.plugin_id.cmp(&b.item.plugin_id))
    });
    ranked
}

fn competitive_uninstalled_candidates(
    ranked: Vec<RankedCandidate>,
    installed_ids: &HashSet<String>,
    max_results: usize,
) -> Vec<SkillMarketCandidate> {
    let top_uninstalled_score = ranked
        .iter()
        .find(|candidate| {
            candidate.score >= MIN_MATCH_SCORE && !installed_ids.contains(&candidate.item.plugin_id)
        })
        .map(|candidate| candidate.score)
        .unwrap_or(0);

    ranked
        .into_iter()
        .filter(|candidate| candidate.score >= MIN_MATCH_SCORE)
        .filter(|candidate| !installed_ids.contains(&candidate.item.plugin_id))
        .filter(|candidate| candidate_is_competitive(candidate, top_uninstalled_score))
        .take(max_results)
        .map(to_search_candidate)
        .collect()
}

fn candidate_is_competitive(candidate: &RankedCandidate, top_score: i64) -> bool {
    if top_score <= 0 {
        return false;
    }
    candidate.score * 100 >= top_score * COMPETITIVE_SCORE_RATIO_PERCENT
}

fn best_installed_match<'a>(
    ranked: &'a [RankedCandidate],
    installed_ids: &HashSet<String>,
    enablement: &SkillEnablementState,
) -> Option<&'a RankedCandidate> {
    let top = ranked.first()?;
    if top.score >= MIN_MATCH_SCORE
        && installed_ids.contains(&top.item.plugin_id)
        && enablement.is_enabled(&top.item.plugin_id)
    {
        Some(top)
    } else {
        None
    }
}

fn best_disabled_installed_match<'a>(
    ranked: &'a [RankedCandidate],
    installed_ids: &HashSet<String>,
    enablement: &SkillEnablementState,
) -> Option<&'a RankedCandidate> {
    let top = ranked.first()?;
    if top.score >= DISABLED_INSTALLED_BLOCK_SCORE
        && installed_ids.contains(&top.item.plugin_id)
        && !enablement.is_enabled(&top.item.plugin_id)
    {
        Some(top)
    } else {
        None
    }
}

fn best_disabled_local_skill_match(
    skills: &[DiskSkill],
    task: &str,
    capability_hints: &[String],
    enablement: &SkillEnablementState,
) -> Option<LocalSkillMatch> {
    let local_items = skills
        .iter()
        .filter(|skill| !enablement.is_enabled(&skill.id))
        .map(local_skill_to_marketplace_item)
        .collect::<Vec<_>>();
    rank_marketplace_candidates(&local_items, task, capability_hints)
        .into_iter()
        .filter(|candidate| candidate.score >= DISABLED_INSTALLED_BLOCK_SCORE)
        .map(|candidate| LocalSkillMatch {
            skill_id: candidate.item.plugin_id,
        })
        .next()
}

fn local_skill_to_marketplace_item(skill: &DiskSkill) -> MarketplaceSkillItem {
    let label = skill
        .frontmatter
        .metadata
        .label
        .clone()
        .unwrap_or_else(|| skill.frontmatter.name.clone());
    let description = match &skill.frontmatter.when_to_use {
        Some(when_to_use) if !when_to_use.trim().is_empty() => {
            format!("{} {}", skill.frontmatter.description, when_to_use)
        }
        _ => skill.frontmatter.description.clone(),
    };
    MarketplaceSkillItem {
        id: 0,
        plugin_id: skill.id.clone(),
        name: label,
        description,
        icon: String::new(),
        version: skill.frontmatter.version.clone().unwrap_or_default(),
        category: skill.frontmatter.category.clone().unwrap_or_default(),
        scope: "local".to_string(),
        status: String::new(),
        downloads: 0,
        featured: false,
        package_size: 0,
        tenant_name: String::new(),
        created_at: String::new(),
    }
}

fn find_skills_loaded_for_current_conversation(ctx: &ToolExecutionContext) -> bool {
    let Some(conv_dir) = ctx.conv_dir.as_ref() else {
        return true;
    };
    let Ok(messages) = std::fs::read_to_string(conv_dir.join("messages.jsonl")) else {
        return true;
    };
    messages_text_contains_find_skills_skill_call(&messages)
}

fn messages_text_contains_find_skills_skill_call(messages: &str) -> bool {
    messages.lines().any(|line| {
        let json_part = line.split('\t').next().unwrap_or(line).trim();
        let Ok(value) = serde_json::from_str::<Value>(json_part) else {
            return false;
        };
        value
            .get("toolCalls")
            .and_then(Value::as_array)
            .map(|tool_calls| {
                tool_calls.iter().any(|call| {
                    if call.get("name").and_then(Value::as_str) != Some("Skill") {
                        return false;
                    }
                    let Some(arguments) = call.get("arguments") else {
                        return false;
                    };
                    arguments
                        .get("skill_id")
                        .or_else(|| arguments.get("skillId"))
                        .and_then(Value::as_str)
                        == Some("find-skills")
                })
            })
            .unwrap_or(false)
    })
}

fn search_status_for_candidate_count(count: usize) -> (&'static str, &'static str) {
    match count {
        0 => ("no_match", "未找到足够匹配的市场技能。不要安装无关技能。"),
        1 => (
            "matched",
            "找到一个候选。只有当 confidence 为 high 且理由明显匹配时才可安装。",
        ),
        _ => (
            "needs_choice",
            "找到多个候选。不要直接安装；必须先调用 AskUserQuestion 让用户选择。",
        ),
    }
}

fn score_item(
    item: &MarketplaceSkillItem,
    terms: &[String],
    capability_hints: &[String],
) -> (i64, Vec<&'static str>) {
    let plugin_id = normalize(&item.plugin_id);
    let name = normalize(&item.name);
    let description = normalize(&item.description);
    let category = normalize(&item.category);
    let mut score = 0;
    let mut reasons = Vec::new();

    for term in terms {
        if term.is_empty() {
            continue;
        }
        if plugin_id == *term || name == *term {
            score += 80;
            push_reason(&mut reasons, "name_match");
        } else if term_can_score_name_contains(term)
            && (plugin_id.contains(term) || name.contains(term))
        {
            score += 36;
            push_reason(&mut reasons, "name_match");
        }
        if term_can_score_body_field(term) && description.contains(term) {
            score += 14;
            push_reason(&mut reasons, "description_match");
        }
        if term_can_score_body_field(term) && category.contains(term) {
            score += 12;
            push_reason(&mut reasons, "category_match");
        }
    }

    for hint in capability_hints {
        let hint = normalize(hint);
        if hint.is_empty() {
            continue;
        }
        if plugin_id.contains(&hint) || name.contains(&hint) || category.contains(&hint) {
            score += 18;
            push_reason(&mut reasons, "capability_match");
        } else if term_can_score_body_field(&hint) && description.contains(&hint) {
            score += 10;
            push_reason(&mut reasons, "capability_match");
        }
    }

    if item.featured && score > 0 {
        score += 3;
    }
    if item.downloads > 0 && score > 0 {
        score += (item.downloads / 100).clamp(0, 5);
    }

    (score, reasons)
}

fn search_terms(task: &str, capability_hints: &[String]) -> Vec<String> {
    let mut terms = split_terms(task);
    terms.extend(cjk_ngram_terms(task));
    for hint in capability_hints {
        terms.extend(split_terms(hint));
        terms.extend(cjk_ngram_terms(hint));
    }
    terms.sort();
    terms.dedup();
    terms
}

fn split_terms(input: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut buf = String::new();
    for c in input.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() || c == '-' || c == '_' || is_cjk(c) {
            buf.push(c);
        } else if !buf.is_empty() {
            terms.push(buf.clone());
            buf.clear();
        }
    }
    if !buf.is_empty() {
        terms.push(buf);
    }
    terms
}

fn cjk_ngram_terms(input: &str) -> Vec<String> {
    const MIN_NGRAM: usize = 2;
    const MAX_NGRAM: usize = 6;

    let mut terms = Vec::new();
    let mut run = Vec::new();
    for c in input.chars().flat_map(char::to_lowercase) {
        if is_cjk(c) {
            run.push(c);
        } else {
            push_cjk_ngrams(&run, &mut terms, MIN_NGRAM, MAX_NGRAM);
            run.clear();
        }
    }
    push_cjk_ngrams(&run, &mut terms, MIN_NGRAM, MAX_NGRAM);
    terms
}

fn push_cjk_ngrams(run: &[char], terms: &mut Vec<String>, min_len: usize, max_len: usize) {
    if run.len() < min_len {
        return;
    }
    for start in 0..run.len() {
        let remaining = run.len() - start;
        for len in min_len..=max_len.min(remaining) {
            terms.push(run[start..start + len].iter().collect());
        }
    }
}

fn normalize(input: &str) -> String {
    input
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || is_cjk(*c))
        .collect()
}

fn term_can_score_body_field(term: &str) -> bool {
    term.chars().count() >= 3
}

fn term_can_score_name_contains(term: &str) -> bool {
    !matches!(term, "分析")
}

fn is_cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

fn confidence(score: i64) -> &'static str {
    if score >= 70 {
        "high"
    } else if score >= 30 {
        "medium"
    } else {
        "low"
    }
}

fn first_chars(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

fn push_reason(reasons: &mut Vec<&'static str>, reason: &'static str) {
    if !reasons.contains(&reason) {
        reasons.push(reason);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::skill::types::{DiskSkill, SkillFrontmatter, SkillMetadata, SkillSource};
    use std::path::PathBuf;

    fn item(id: i64, plugin_id: &str, name: &str, description: &str) -> MarketplaceSkillItem {
        MarketplaceSkillItem {
            id,
            plugin_id: plugin_id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            category: String::new(),
            icon: String::new(),
            version: String::new(),
            scope: String::new(),
            status: String::new(),
            downloads: 0,
            featured: false,
            package_size: 0,
            tenant_name: String::new(),
            created_at: String::new(),
        }
    }

    fn disk_skill(id: &str, label: &str, description: &str, when_to_use: &str) -> DiskSkill {
        DiskSkill {
            id: id.to_string(),
            root: PathBuf::from("/tmp"),
            frontmatter: SkillFrontmatter {
                name: id.to_string(),
                description: description.to_string(),
                when_to_use: Some(when_to_use.to_string()),
                allowed_tools: vec![],
                argument_hint: None,
                arguments: vec![],
                model: None,
                effort: None,
                context: None,
                agent: None,
                user_invocable: true,
                disable_model_invocation: false,
                version: None,
                paths: vec![],
                hooks: Default::default(),
                shell: None,
                category: None,
                metadata: SkillMetadata {
                    label: Some(label.to_string()),
                    display_i18n: Default::default(),
                },
            },
            body: String::new(),
            source: SkillSource::User,
        }
    }

    #[test]
    fn rank_prefers_name_and_description_matches() {
        let items = vec![
            item(1, "pdf-helper", "PDF", "读取 PDF 文件"),
            item(
                2,
                "browser-scraper",
                "浏览器网页抓取",
                "访问网页、浏览器自动化、抓取网页数据",
            ),
        ];

        let ranked = rank_marketplace_candidates(
            &items,
            "访问网站抓一下数据",
            &["browser".to_string(), "web_scraping".to_string()],
        );

        assert_eq!(ranked[0].item.plugin_id, "browser-scraper");
        assert!(ranked[0].score >= MIN_MATCH_SCORE);
    }

    #[test]
    fn no_match_scores_zero() {
        let items = vec![item(1, "pdf-helper", "PDF", "读取 PDF 文件")];

        let ranked = rank_marketplace_candidates(&items, "__no_match_9f3b__", &[]);

        assert!(ranked.is_empty());
    }

    #[test]
    fn search_terms_do_not_expand_to_product_specific_skill_ids() {
        let terms = search_terms("帮我看看甲辰流程今天有哪些待办", &[]);

        assert!(terms.contains(&"甲辰".to_string()));
        assert!(terms.contains(&"流程".to_string()));
        assert!(terms.contains(&"待办".to_string()));
        assert!(!terms.contains(&"alpha-workflow".to_string()));
        assert!(!terms.contains(&"workflow-helper".to_string()));
    }

    #[test]
    fn cjk_terms_can_match_new_market_skill_without_alias_table() {
        let items = vec![
            item(
                1,
                "browser",
                "浏览器自动化",
                "打开网页、点击、填表、抓取页面数据",
            ),
            item(
                2,
                "alpha-workflow",
                "甲辰流程助手",
                "处理甲辰流程待办、流程汇总和进度查询",
            ),
        ];

        let ranked = rank_marketplace_candidates(&items, "帮我看看甲辰流程里有没有待处理流程", &[]);

        assert_eq!(ranked[0].item.plugin_id, "alpha-workflow");
        assert!(ranked[0].score >= MIN_MATCH_SCORE);
    }

    #[test]
    fn natural_cjk_task_prefers_dedicated_skill_without_hints() {
        let items = vec![
            item(
                1,
                "browser",
                "浏览器自动化",
                "打开网页、点击、填表、抓取页面数据",
            ),
            item(
                2,
                "workflow-hub",
                "协同待办助手",
                "管理协同待办、日程、消息和审批流程",
            ),
        ];

        let ranked = rank_marketplace_candidates(&items, "帮我看看协同待办今天有哪些事项", &[]);

        assert_eq!(ranked[0].item.plugin_id, "workflow-hub");
        assert!(ranked[0].score >= MIN_MATCH_SCORE);
    }

    #[test]
    fn metadata_description_matches_people_lookup_without_alias_table() {
        let items = vec![item(
            1,
            "people-directory",
            "人员资料助手",
            "查询员工部门、岗位、职级、组织架构和人员信息",
        )];

        let ranked = rank_marketplace_candidates(&items, "帮我查王小明部门和岗位", &[]);

        assert_eq!(ranked[0].item.plugin_id, "people-directory");
        assert!(ranked[0].score >= MIN_MATCH_SCORE);
    }

    #[test]
    fn installed_generic_skill_does_not_hide_better_market_candidate() {
        let items = vec![
            item(
                1,
                "browser",
                "浏览器自动化",
                "打开网页、点击、填表、抓取页面数据",
            ),
            item(
                2,
                "workflow-hub",
                "协同待办助手",
                "管理协同待办、日程、消息和审批流程",
            ),
        ];
        let ranked = rank_marketplace_candidates(&items, "打开协同待办网页查看待办", &[]);
        let installed = HashSet::from(["browser".to_string()]);
        let enablement = SkillEnablementState::default();

        assert_eq!(ranked[0].item.plugin_id, "workflow-hub");
        assert!(best_installed_match(&ranked, &installed, &enablement).is_none());
    }

    #[test]
    fn lower_ranked_installed_match_does_not_return_already_installed() {
        let ranked = vec![
            RankedCandidate {
                item: item(
                    1,
                    "event-path-lab",
                    "甲辰事件路径分析",
                    "分析甲辰事件日志、路径、功能使用、活跃度和留存。",
                ),
                score: 120,
                reasons: vec!["description_match"],
            },
            RankedCandidate {
                item: item(
                    2,
                    "benchmark-lab",
                    "乙类对标分析",
                    "对表格结构、区间对标和调整建议进行分析。",
                ),
                score: 28,
                reasons: vec!["description_match"],
            },
        ];
        let installed = HashSet::from(["benchmark-lab".to_string()]);
        let enablement = SkillEnablementState::default();

        assert!(best_installed_match(&ranked, &installed, &enablement).is_none());
    }

    #[test]
    fn top_ranked_installed_match_returns_already_installed() {
        let ranked = vec![
            RankedCandidate {
                item: item(
                    1,
                    "event-path-lab",
                    "甲辰事件路径分析",
                    "分析甲辰事件日志、路径、功能使用、活跃度和留存。",
                ),
                score: 120,
                reasons: vec!["description_match"],
            },
            RankedCandidate {
                item: item(
                    2,
                    "benchmark-lab",
                    "乙类对标分析",
                    "对表格结构、区间对标和调整建议进行分析。",
                ),
                score: 28,
                reasons: vec!["description_match"],
            },
        ];
        let installed = HashSet::from(["event-path-lab".to_string()]);
        let enablement = SkillEnablementState::default();

        assert_eq!(
            best_installed_match(&ranked, &installed, &enablement)
                .unwrap()
                .item
                .plugin_id,
            "event-path-lab"
        );
    }

    #[test]
    fn dedicated_market_task_beats_installed_browser_hint() {
        let items = vec![
            item(
                1,
                "browser",
                "浏览器自动化",
                "打开网页、点击、填表、抓取页面数据",
            ),
            item(
                2,
                "batch-helper",
                "结算批次助手",
                "管理结算批次、统计、概览、生成状态和档案",
            ),
        ];
        let ranked = rank_marketplace_candidates(
            &items,
            "用结算批次助手查本月批次概览",
            &[
                "batch_management".to_string(),
                "browser_automation".to_string(),
            ],
        );
        let installed = HashSet::from(["browser".to_string()]);
        let enablement = SkillEnablementState::default();

        assert_eq!(ranked[0].item.plugin_id, "batch-helper");
        assert!(ranked[0].score >= MIN_MATCH_SCORE);
        assert!(best_installed_match(&ranked, &installed, &enablement).is_none());
    }

    #[test]
    fn analysis_task_matches_multiple_analysis_candidates() {
        let items = vec![
            item(
                1,
                "analysis-primary",
                "专项公平分析",
                "专项公平分析——对业务表进行数据清洗、分组归一化、指标诊断，并生成调整建议。",
            ),
            item(
                2,
                "analysis-benchmark",
                "专项对标分析",
                "用于在用户提供业务表、分组、等级或对标数据后，完成内部结构、区间对标、竞争力和调整建议分析。",
            ),
        ];

        let ranked = rank_marketplace_candidates(&items, "业务表公平分析和调整建议", &[]);

        assert_eq!(ranked[0].item.plugin_id, "analysis-primary");
        assert!(ranked
            .iter()
            .any(|candidate| candidate.item.plugin_id == "analysis-benchmark"));
    }

    #[test]
    fn short_cjk_generic_terms_do_not_promote_unrelated_analysis_candidate() {
        let items = vec![
            item(
                1,
                "event-path-lab",
                "甲辰事件路径分析",
                "分析甲辰事件日志、路径、功能使用、活跃度和留存。",
            ),
            item(
                2,
                "benchmark-lab",
                "乙类对标分析",
                "当用户提供业务表后，进行数据清洗、区间对标和分析建议。",
            ),
        ];

        let ranked = rank_marketplace_candidates(
            &items,
            "我有一份甲辰事件日志，想分析路径、活跃度和留存",
            &[],
        );

        assert_eq!(ranked[0].item.plugin_id, "event-path-lab");
        assert!(!ranked
            .iter()
            .any(|candidate| candidate.item.plugin_id == "benchmark-lab"
                && candidate.score >= MIN_MATCH_SCORE));
    }

    #[test]
    fn dominant_candidate_suppresses_generic_lower_scored_candidates() {
        let ranked = vec![
            RankedCandidate {
                item: item(
                    1,
                    "event-path-lab",
                    "甲辰事件路径分析",
                    "分析甲辰事件日志、路径、功能使用、活跃度和留存。",
                ),
                score: 900,
                reasons: vec!["name_match", "description_match"],
            },
            RankedCandidate {
                item: item(
                    2,
                    "operations-lab",
                    "综合数据分析",
                    "用于分析综合运营数据、活跃、留存和增长策略。",
                ),
                score: 220,
                reasons: vec!["description_match"],
            },
        ];
        let installed = HashSet::new();

        let candidates = competitive_uninstalled_candidates(ranked, &installed, 5);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].plugin_id, "event-path-lab");
    }

    #[test]
    fn disabled_installed_skill_is_reported_as_blocked_not_usable() {
        let items = vec![item(
            2,
            "batch-helper",
            "结算批次助手",
            "管理结算批次、统计、概览、生成状态和档案",
        )];
        let ranked = rank_marketplace_candidates(&items, "这个月哪些结算批次已经生成了", &[]);
        let installed = HashSet::from(["batch-helper".to_string()]);
        let mut enablement = crate::plugin::skill::enablement::SkillEnablementState::default();
        enablement
            .disabled_skill_ids
            .insert("batch-helper".to_string());

        assert!(best_installed_match(&ranked, &installed, &enablement).is_none());
        assert_eq!(
            best_disabled_installed_match(&ranked, &installed, &enablement)
                .unwrap()
                .item
                .plugin_id,
            "batch-helper"
        );
    }

    #[test]
    fn disabled_installed_skill_is_not_bypassed_by_enabled_installed_candidate_ranked_first() {
        let ranked = vec![
            RankedCandidate {
                item: item(
                    1,
                    "people-directory",
                    "人员资料助手",
                    "通过命令行管理员工、部门、组织、岗位和人员信息",
                ),
                score: 60,
                reasons: vec!["capability_match"],
            },
            RankedCandidate {
                item: item(
                    2,
                    "batch-helper",
                    "结算批次助手",
                    "管理结算批次、统计、概览、生成状态和档案",
                ),
                score: 80,
                reasons: vec!["description_match"],
            },
        ];
        let installed = HashSet::from(["people-directory".to_string(), "batch-helper".to_string()]);
        let mut enablement = crate::plugin::skill::enablement::SkillEnablementState::default();
        enablement
            .disabled_skill_ids
            .insert("batch-helper".to_string());

        assert_eq!(
            best_disabled_installed_match(&ranked, &installed, &enablement)
                .unwrap()
                .item
                .plugin_id,
            "batch-helper"
        );
    }

    #[test]
    fn disabled_local_skill_match_uses_installed_registry_metadata() {
        let skills = vec![
            disk_skill(
                "people-directory",
                "人员资料助手",
                "通过命令行管理员工入职、花名册、组织、职位等人员资料业务。",
                "当用户提到员工花名册、组织/职位/职级等任务时使用。",
            ),
            disk_skill(
                "batch-helper",
                "结算批次助手",
                "通过命令行管理结算批次、统计、概览、生成状态和档案。",
                "当用户提到结算批次、生成状态、批次概览等任务时使用。",
            ),
        ];
        let mut enablement = crate::plugin::skill::enablement::SkillEnablementState::default();
        enablement
            .disabled_skill_ids
            .insert("batch-helper".to_string());

        let matched = best_disabled_local_skill_match(
            &skills,
            "帮我用结算批次助手查一下本月批次概览，只读查看",
            &[
                "people_directory".to_string(),
                "browser_automation".to_string(),
                "ops".to_string(),
                "batch_management".to_string(),
            ],
            &enablement,
        )
        .unwrap();

        assert_eq!(matched.skill_id, "batch-helper");
    }

    #[test]
    fn generic_analysis_does_not_block_on_disabled_batch_helper() {
        let skills = vec![disk_skill(
            "batch-helper",
            "结算批次助手",
            "通过命令行管理结算批次、统计、概览、生成状态和档案。",
            "当用户提到结算批次、生成状态、批次概览等任务时使用。",
        )];
        let mut enablement = crate::plugin::skill::enablement::SkillEnablementState::default();
        enablement
            .disabled_skill_ids
            .insert("batch-helper".to_string());

        assert!(best_disabled_local_skill_match(
            &skills,
            "业务表公平分析和调整建议",
            &["业务表".to_string(), "公平分析".to_string()],
            &enablement,
        )
        .is_none());
    }

    #[test]
    fn detects_find_skills_loaded_from_tool_call_history() {
        let messages = r#"{"role":"assistant","content":{"text":"需要 find-skills"},"toolCalls":[{"name":"Skill","arguments":{"skill_id":"find-skills"}}]}	✓
{"role":"tool","name":"Skill","content":{"text":"find-skills body"}}"#;

        assert!(messages_text_contains_find_skills_skill_call(messages));
    }

    #[test]
    fn plain_text_find_skills_does_not_count_as_loaded() {
        let messages = r#"{"role":"assistant","content":{"text":"我应该使用 find-skills"},"toolCalls":[{"name":"SkillMarketSearch","arguments":{"task":"find-skills"}}]}	✓"#;

        assert!(!messages_text_contains_find_skills_skill_call(messages));
    }

    #[test]
    fn multiple_candidates_require_user_choice() {
        assert_eq!(search_status_for_candidate_count(0).0, "no_match");
        assert_eq!(search_status_for_candidate_count(1).0, "matched");
        assert_eq!(search_status_for_candidate_count(2).0, "needs_choice");
    }
}
