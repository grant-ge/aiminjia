use tauri::AppHandle;
use tauri::Emitter;
use tauri::Manager;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use notify::{Watcher, RecursiveMode, RecommendedWatcher};

/// Global storage for the dev-mode file watcher.
/// Only one skill can be watched at a time.
static DEV_WATCHER: once_cell::sync::Lazy<std::sync::Mutex<Option<RecommendedWatcher>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(None));

#[derive(serde::Serialize)]
pub struct CustomSkillInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub path: String,
    pub enabled: bool,
}

/// List all installed custom plugins.
#[tauri::command]
pub async fn list_custom_skills(app: AppHandle) -> Result<Vec<CustomSkillInfo>, String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let custom_dir = app_data.join("custom_plugins");

    if !custom_dir.is_dir() {
        return Ok(vec![]);
    }

    let mut skills = Vec::new();
    for entry in std::fs::read_dir(&custom_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() && !path.file_name().unwrap().to_string_lossy().starts_with('_') {
            let manifest_path = path.join("plugin.toml");
            if manifest_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                    if let Ok(manifest) = toml::from_str::<toml::Value>(&content) {
                        let plugin = manifest
                            .get("plugin")
                            .cloned()
                            .unwrap_or(toml::Value::Table(Default::default()));
                        skills.push(CustomSkillInfo {
                            id: plugin
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            name: plugin
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            description: plugin
                                .get("description")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            path: path.to_string_lossy().to_string(),
                            enabled: !path
                                .file_name()
                                .unwrap()
                                .to_string_lossy()
                                .starts_with('_'),
                        });
                    }
                }
            }
        }
    }
    Ok(skills)
}

/// Install a skill from a directory path (copy to custom_plugins/).
#[tauri::command]
pub async fn install_custom_skill(
    app: AppHandle,
    source_path: String,
) -> Result<String, String> {
    let source = PathBuf::from(&source_path);
    if !source.is_dir() {
        return Err("Source path is not a directory".to_string());
    }

    let manifest_path = source.join("plugin.toml");
    if !manifest_path.exists() {
        return Err("No plugin.toml found in source directory".to_string());
    }

    // Read plugin ID from manifest
    let content = std::fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
    let manifest: toml::Value = toml::from_str(&content).map_err(|e| e.to_string())?;
    let plugin_id = manifest
        .get("plugin")
        .and_then(|p| p.get("id"))
        .and_then(|v| v.as_str())
        .ok_or("plugin.id not found in manifest")?
        .to_string();

    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let custom_dir = app_data.join("custom_plugins");
    std::fs::create_dir_all(&custom_dir).map_err(|e| e.to_string())?;

    let dest = custom_dir.join(&plugin_id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
    }

    // Copy directory recursively
    copy_dir_recursive(&source, &dest).map_err(|e| e.to_string())?;

    Ok(format!(
        "Installed skill '{}' — restart app to activate",
        plugin_id
    ))
}

/// Uninstall a custom skill by ID.
#[tauri::command]
pub async fn uninstall_custom_skill(
    app: AppHandle,
    skill_id: String,
) -> Result<String, String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let skill_dir = app_data.join("custom_plugins").join(&skill_id);

    if !skill_dir.exists() {
        return Err(format!("Custom skill '{}' not found", skill_id));
    }

    std::fs::remove_dir_all(&skill_dir).map_err(|e| e.to_string())?;
    Ok(format!(
        "Uninstalled skill '{}' — restart app to take effect",
        skill_id
    ))
}

/// Create a new skill template directory with scaffolding files.
#[tauri::command]
pub async fn init_skill_template(target_dir: String, skill_id: String, skill_name: String) -> Result<String, String> {
    let dir = PathBuf::from(&target_dir).join(&skill_id);
    if dir.exists() {
        return Err(format!("Directory '{}' already exists", dir.display()));
    }

    // Create directory structure
    std::fs::create_dir_all(dir.join("prompts")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("scripts/knowledge")).map_err(|e| e.to_string())?;

    // plugin.toml
    let plugin_toml = format!(r#"[plugin]
id = "{skill_id}"
name = "{skill_name}"
type = "skill"
description = ""
priority = 20

[trigger]
keywords = ["{skill_name}"]
requires_files = false

[model]
preference = "deep_reasoning"

[prompts]
include_app_base = true

[defaults]
max_iterations = 5
token_budget = 8192

[display]
category = "general"
icon = "🔧"
short_description = ""
trigger_text = ""
"#);
    std::fs::write(dir.join("plugin.toml"), plugin_toml).map_err(|e| e.to_string())?;

    // workflow.toml
    let workflow_toml = r#"[[steps]]
id = "step0"
name = "信息采集"
prompt = "prompts/step0.md"
precompute = "scripts/step0.py"
tools_only = ["save_analysis_note"]
max_iterations = 5
token_budget = 8192
advance_on = "any"

[[steps]]
id = "step1"
name = "分析处理"
prompt = "prompts/step1.md"
tools_only = ["execute_python", "export_data"]
max_iterations = 5
token_budget = 8192
advance_on = "confirm"

[steps.tools_on_feedback]
tools = ["execute_python", "export_data"]
max_iterations = 15

[[steps]]
id = "step2"
name = "报告生成"
prompt = "prompts/step2.md"
tools_only = ["generate_report", "export_data"]
max_iterations = 5
token_budget = 8192
advance_on = "confirm"
"#;
    std::fs::write(dir.join("workflow.toml"), workflow_toml).map_err(|e| e.to_string())?;

    // prompts/base.md
    std::fs::write(dir.join("prompts/base.md"), format!("# {skill_name}\n\n你是{skill_name}专家。\n")).map_err(|e| e.to_string())?;

    // prompts/step0.md
    std::fs::write(dir.join("prompts/step0.md"), r#"# Step 0: 信息采集

系统已自动加载知识库，结果在 [precompute_result] 中。

**如果 [precompute_result] 存在且有效：**
- 展示知识库内容，确认分析方向

**如果 [precompute_result] 不存在或出错：**
- 向用户收集必要信息

确认后进入下一步。
"#).map_err(|e| e.to_string())?;

    // prompts/step1.md
    std::fs::write(dir.join("prompts/step1.md"), "# Step 1: 分析处理\n\n基于 Step 0 确认的信息，执行分析。\n\n展示分析结果后等待用户确认。\n").map_err(|e| e.to_string())?;

    // prompts/step2.md
    std::fs::write(dir.join("prompts/step2.md"), "# Step 2: 报告生成\n\n综合所有分析结果，生成最终报告。\n\n使用 `generate_report` 生成 HTML 报告。\n使用 `export_data` 导出数据明细。\n").map_err(|e| e.to_string())?;

    // scripts/step0.py
    std::fs::write(dir.join("scripts/step0.py"), r#"import json as _json_mod
import os as _os_mod

result = {}
try:
    # Load knowledge base
    _knowledge = _KNOWLEDGE if '_KNOWLEDGE' in dir() else {}
    result = {
        'knowledge_loaded': bool(_knowledge),
        'available_keys': list(_knowledge.keys()) if _knowledge else [],
        'note': 'Knowledge base loaded successfully' if _knowledge else 'No knowledge files found'
    }
except Exception as e:
    result = {'error': str(e)}

with open(_os_mod.path.join(_ANALYSIS_DIR, 'step0_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, indent=2))
"#).map_err(|e| e.to_string())?;

    // scripts/knowledge/templates.json (example)
    std::fs::write(dir.join("scripts/knowledge/templates.json"), "{\n  \"example_key\": \"Replace with your domain knowledge\"\n}\n").map_err(|e| e.to_string())?;

    // README.md
    let readme = format!(r#"# {skill_name}

## 目录结构

```
{skill_id}/
├── plugin.toml              # 技能元数据
├── workflow.toml             # 工作流定义
├── prompts/                  # LLM 提示词
│   ├── base.md
│   ├── step0.md
│   ├── step1.md
│   └── step2.md
├── scripts/                  # Precompute 脚本
│   ├── step0.py
│   └── knowledge/            # 知识库
│       └── templates.json
└── README.md
```

## 开发

1. 编辑 `plugin.toml` 填写触发关键词和描述
2. 在 `scripts/knowledge/` 中添加领域知识 JSON 文件
3. 编辑 `prompts/*.md` 定义每步的 LLM 行为
4. 编辑 `scripts/*.py` 实现数据处理逻辑
5. 在 AI小家 设置 → 技能管理 → 安装技能，选择此目录
6. 重启应用测试

## 知识库

在 `scripts/knowledge/` 中放置 JSON 文件，precompute 脚本可通过 `_KNOWLEDGE` dict 访问：

```python
_data = _KNOWLEDGE.get('templates', {{}}) if '_KNOWLEDGE' in dir() else {{}}
```
"#);
    std::fs::write(dir.join("README.md"), readme).map_err(|e| e.to_string())?;

    Ok(dir.to_string_lossy().to_string())
}

/// Pack a skill directory into a .aijia-skill zip file in its parent dir.
/// Thin wrapper over `pack_skill_to_dir`.
#[tauri::command]
pub async fn pack_skill(skill_dir: String) -> Result<String, String> {
    let dir = PathBuf::from(&skill_dir);
    let parent = dir.parent().unwrap_or(&dir).to_path_buf();
    let output = pack_skill_to_dir(&dir, &parent)?;
    Ok(output.to_string_lossy().to_string())
}

/// Reload a custom skill from disk (hot-reload for dev mode).
/// Re-reads plugin.toml, unregisters the old version, and registers the new one.
#[tauri::command]
pub async fn reload_skill(
    app: AppHandle,
    skill_path: String,
) -> Result<String, String> {
    let path = PathBuf::from(&skill_path);
    if !path.join("plugin.toml").exists() {
        return Err("No plugin.toml found".to_string());
    }

    // Parse manifest
    let manifest_content = std::fs::read_to_string(path.join("plugin.toml"))
        .map_err(|e| e.to_string())?;
    let manifest = crate::plugin::manifest::parse_plugin_manifest(&manifest_content)
        .map_err(|e| e.to_string())?;

    let plugin_id = manifest.plugin.id.clone();

    // Load the skill
    let skill = crate::plugin::declarative_skill::DeclarativeSkill::load(&manifest, &path)
        .map_err(|e| e.to_string())?;

    // Get registry from app state and replace
    let registry = app.state::<Arc<crate::plugin::SkillRegistry>>();

    // Unregister old version (if exists), then register new version
    registry.unregister(&plugin_id).await;
    registry.register(Arc::new(skill), "custom").await;

    log::info!("Dev mode: reloaded skill '{}'", plugin_id);
    Ok(format!("Skill '{}' reloaded", plugin_id))
}

/// Start watching a skill directory for file changes (dev mode).
/// Emits `skill-file-changed` Tauri event when files are modified.
#[tauri::command]
pub async fn start_skill_watch(
    app: AppHandle,
    skill_path: String,
) -> Result<String, String> {
    let path = PathBuf::from(&skill_path);
    if !path.is_dir() {
        return Err("Not a valid directory".to_string());
    }

    let app_clone = app.clone();
    let path_str = skill_path.clone();

    let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
        if let Ok(event) = res {
            // Only emit on content changes (modify/create), not on access or removal
            if event.kind.is_modify() || event.kind.is_create() {
                let _ = app_clone.emit("skill-file-changed", &path_str);
            }
        }
    }).map_err(|e| e.to_string())?;

    watcher.watch(&path, RecursiveMode::Recursive).map_err(|e| e.to_string())?;

    // Store the watcher (drops any previous watcher, stopping its watch)
    *DEV_WATCHER.lock().unwrap_or_else(|e| e.into_inner()) = Some(watcher);

    log::info!("Dev mode: watching skill directory '{}'", path.display());
    Ok(format!("Watching '{}'", path.display()))
}

/// Stop watching the skill directory (dev mode).
#[tauri::command]
pub async fn stop_skill_watch() -> Result<String, String> {
    *DEV_WATCHER.lock().unwrap_or_else(|e| e.into_inner()) = None;
    log::info!("Dev mode: stopped watching skill directory");
    Ok("Stopped watching".to_string())
}

// ---------------------------------------------------------------------------
// Marketplace Commands
// ---------------------------------------------------------------------------

/// Marketplace skill package returned from the API.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceSkillItem {
    pub id: i64,
    pub plugin_id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub icon: String,
    pub version: String,
    pub scope: String,
    pub status: String,
    pub downloads: i64,
    pub featured: bool,
    pub package_size: i64,
    pub tenant_name: String,
    pub created_at: String,
}

/// Paginated response from the marketplace API.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceResponse {
    pub items: Vec<MarketplaceSkillItem>,
    pub total: i64,
    pub page: i64,
    pub size: i64,
}

/// List skill packages from the cloud marketplace.
#[tauri::command]
pub async fn list_marketplace_skills(
    auth: tauri::State<'_, Arc<crate::auth::AuthManager>>,
    page: u32,
    size: u32,
    category: Option<String>,
    search: Option<String>,
) -> Result<MarketplaceResponse, String> {
    let session_key = auth.get_session_key().await.map_err(|e| e.to_string())?;

    let client = reqwest::Client::new();
    let mut url = format!(
        "https://ai-tenant.renlijia.com/v1/skill-packages?page={}&size={}&scope=public",
        page, size
    );
    if let Some(cat) = &category {
        if !cat.is_empty() {
            url.push_str(&format!("&category={}", cat));
        }
    }
    if let Some(q) = &search {
        if !q.is_empty() {
            // Simple percent-encode for CJK search terms
            let encoded: String = q.chars().map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~' {
                    c.to_string()
                } else {
                    let mut buf = [0u8; 4];
                    c.encode_utf8(&mut buf);
                    buf[..c.len_utf8()].iter().map(|b| format!("%{:02X}", b)).collect()
                }
            }).collect();
            url.push_str(&format!("&search={}", encoded));
        }
    }

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", session_key))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("API error ({}): {}", status, body));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    // Parse { code: 0, data: { items, total, page, size } }
    let data = &body["data"];
    let items: Vec<MarketplaceSkillItem> = serde_json::from_value(
        data["items"].clone()
    ).unwrap_or_default();

    Ok(MarketplaceResponse {
        items,
        total: data["total"].as_i64().unwrap_or(0),
        page: data["page"].as_i64().unwrap_or(1),
        size: data["size"].as_i64().unwrap_or(20),
    })
}

/// Download and install a skill package from the marketplace.
/// Downloads the zip from `package_url` and extracts to `custom_plugins/{plugin_id}/`.
#[tauri::command]
pub async fn install_marketplace_skill(
    app: AppHandle,
    auth: tauri::State<'_, Arc<crate::auth::AuthManager>>,
    package_id: i64,
    plugin_id: String,
) -> Result<String, String> {
    let session_key = auth.get_session_key().await.map_err(|e| e.to_string())?;

    let client = reqwest::Client::new();

    // Step 1: Get the download URL
    let download_url = format!(
        "https://ai-tenant.renlijia.com/v1/skill-packages/{}/download",
        package_id
    );
    let resp = client
        .post(&download_url)
        .header("Authorization", format!("Bearer {}", session_key))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Download API error: {}", body));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let package_url = body["package_url"]
        .as_str()
        .or_else(|| body["data"]["package_url"].as_str())
        .ok_or("No package_url in response")?
        .to_string();

    // Step 2: Download the zip file
    let zip_resp = client
        .get(&package_url)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| format!("Download error: {}", e))?;

    if !zip_resp.status().is_success() {
        return Err(format!("Failed to download package: HTTP {}", zip_resp.status()));
    }

    let zip_bytes = zip_resp.bytes().await.map_err(|e| format!("Download error: {}", e))?;

    // Step 3: Extract to custom_plugins/{plugin_id}/
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let custom_dir = app_data.join("custom_plugins");
    std::fs::create_dir_all(&custom_dir).map_err(|e| e.to_string())?;

    let dest = custom_dir.join(&plugin_id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;

    let cursor = std::io::Cursor::new(zip_bytes.as_ref());
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("Invalid zip: {}", e))?;

    const MAX_EXTRACT_SIZE: u64 = 50 * 1024 * 1024; // 50 MB limit
    let mut total_extracted: u64 = 0;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;

        total_extracted += file.size();
        if total_extracted > MAX_EXTRACT_SIZE {
            let _ = std::fs::remove_dir_all(&dest);
            return Err("Package too large (exceeds 50MB extraction limit)".to_string());
        }

        let out_path = dest.join(file.mangled_name());

        if file.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut outfile = std::fs::File::create(&out_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut file, &mut outfile).map_err(|e| e.to_string())?;
        }
    }

    log::info!("Marketplace: installed skill '{}' to {:?}", plugin_id, dest);
    Ok(format!("Installed '{}' — restart app to activate", plugin_id))
}

/// Copy a directory recursively. Symlinks are skipped (path-traversal defense).
/// `pub(crate)` so `skill_smith::commit` can reuse without duplicating logic.
pub(crate) fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        // Skip symlinks to prevent path traversal attacks
        if src_path.is_symlink() {
            log::warn!("Skipping symlink during copy: {}", src_path.display());
            continue;
        }
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Package a skill directory into `{output_dir}/{plugin_id}.aijia-skill` zip.
/// Shared between `pack_skill` (tauri command) and `skill_smith::commit::export_skill_draft`.
pub(crate) fn pack_skill_to_dir(
    skill_dir: &Path,
    output_dir: &Path,
) -> Result<PathBuf, String> {
    if !skill_dir.is_dir() {
        return Err("Skill directory does not exist".to_string());
    }
    if !skill_dir.join("plugin.toml").exists() {
        return Err("No plugin.toml found — not a valid skill directory".to_string());
    }

    // Read plugin ID for output filename
    let manifest_content = std::fs::read_to_string(skill_dir.join("plugin.toml"))
        .map_err(|e| e.to_string())?;
    let manifest: toml::Value = toml::from_str(&manifest_content).map_err(|e| e.to_string())?;
    let plugin_id = manifest
        .get("plugin")
        .and_then(|p| p.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    std::fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;
    let output_path = output_dir.join(format!("{}.aijia-skill", plugin_id));

    let file = std::fs::File::create(&output_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    fn add_dir_to_zip(
        zip: &mut zip::ZipWriter<std::fs::File>,
        dir: &std::path::Path,
        base: &std::path::Path,
        options: zip::write::SimpleFileOptions,
    ) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let relative = path.strip_prefix(base).map_err(|e| e.to_string())?;
            let name = relative.to_string_lossy().to_string();

            if path.is_dir() {
                zip.add_directory(format!("{}/", name), options)
                    .map_err(|e| e.to_string())?;
                add_dir_to_zip(zip, &path, base, options)?;
            } else {
                zip.start_file(&name, options).map_err(|e| e.to_string())?;
                let content = std::fs::read(&path).map_err(|e| e.to_string())?;
                std::io::Write::write_all(zip, &content).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    add_dir_to_zip(&mut zip, skill_dir, skill_dir, options)?;
    zip.finish().map_err(|e| e.to_string())?;

    Ok(output_path)
}
