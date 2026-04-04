use tauri::AppHandle;
use tauri::Manager;
use std::path::PathBuf;

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

/// Pack a skill directory into a .aijia-skill zip file.
#[tauri::command]
pub async fn pack_skill(skill_dir: String) -> Result<String, String> {
    let dir = PathBuf::from(&skill_dir);
    if !dir.is_dir() {
        return Err("Not a valid directory".to_string());
    }
    if !dir.join("plugin.toml").exists() {
        return Err("No plugin.toml found — not a valid skill directory".to_string());
    }

    // Read plugin ID for output filename
    let manifest_content = std::fs::read_to_string(dir.join("plugin.toml")).map_err(|e| e.to_string())?;
    let manifest: toml::Value = toml::from_str(&manifest_content).map_err(|e| e.to_string())?;
    let plugin_id = manifest
        .get("plugin")
        .and_then(|p| p.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let output_path = dir.parent()
        .unwrap_or(&dir)
        .join(format!("{}.aijia-skill", plugin_id));

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
                zip.add_directory(&format!("{}/", name), options).map_err(|e| e.to_string())?;
                add_dir_to_zip(zip, &path, base, options)?;
            } else {
                zip.start_file(&name, options).map_err(|e| e.to_string())?;
                let content = std::fs::read(&path).map_err(|e| e.to_string())?;
                std::io::Write::write_all(zip, &content).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    add_dir_to_zip(&mut zip, &dir, &dir, options)?;
    zip.finish().map_err(|e| e.to_string())?;

    Ok(output_path.to_string_lossy().to_string())
}

fn copy_dir_recursive(src: &PathBuf, dst: &PathBuf) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
