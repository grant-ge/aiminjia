use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::runtime::dependencies::{ManagedRuntimeProcessEnv, RuntimeResolver};

#[cfg(target_os = "windows")]
use crate::storage::process_ext::NoWindowExt;

pub struct SkillSubstitutionContext {
    pub skill_dir: PathBuf,
    pub session_id: String,
    pub args: String,
    pub argument_names: Vec<String>,
    pub execute_shell: bool,
    pub runtime_resolver: Option<Arc<dyn RuntimeResolver>>,
    pub managed_runtime_enabled: bool,
}

pub fn substitute_skill_body(body: &str, ctx: &SkillSubstitutionContext) -> Result<String> {
    let parsed_args = shell_words::split(&ctx.args).unwrap_or_else(|_| {
        ctx.args
            .split_whitespace()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    });

    let mut out = body.to_string();
    out = out.replace("${AIJIA_SKILL_DIR}", &ctx.skill_dir.display().to_string());
    out = out.replace("${AIJIA_SESSION_ID}", &ctx.session_id);
    out = out.replace("$ARGUMENTS", &ctx.args);

    for idx in 0..9 {
        let value = parsed_args.get(idx).cloned().unwrap_or_default();
        out = out.replace(&format!("$ARGUMENTS[{idx}]"), &value);
        out = out.replace(&format!("${}", idx + 1), &value);
    }

    for (idx, name) in ctx.argument_names.iter().enumerate() {
        if let Some(value) = parsed_args.get(idx) {
            out = out.replace(&format!("${name}"), value);
        }
    }

    if !ctx.args.trim().is_empty() && !body.contains("$ARGUMENTS") {
        out.push_str("\n\nARGUMENTS: ");
        out.push_str(&ctx.args);
    }

    if ctx.execute_shell {
        out = execute_inline_shell_blocks(
            &out,
            ctx.runtime_resolver.as_deref(),
            ctx.managed_runtime_enabled,
        )?;
    }

    Ok(out)
}

fn execute_inline_shell_blocks(
    input: &str,
    runtime_resolver: Option<&dyn RuntimeResolver>,
    managed_runtime_enabled: bool,
) -> Result<String> {
    let mut result = String::new();
    let mut rest = input;
    while let Some(start) = rest.find("!`") {
        result.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('`') else {
            result.push_str("!`");
            result.push_str(after);
            return Ok(result);
        };
        let cmd = &after[..end];
        // Windows: route through PowerShell since `bash` is not present unless
        // Git Bash is on PATH. -NoProfile keeps startup snappy. The UTF-8
        // prologue is required so Chinese / non-ASCII output isn't reduced to
        // `?` by the default CP936 console encoding.
        // Silently swallow encoding-setup failures (e.g. ConstrainedLanguage
        // mode forbids property setters on .NET types). See powershell.rs for
        // the same pattern.
        #[cfg(target_os = "windows")]
        let wrapped_cmd = format!(
            "chcp 65001 > $null 2>$null; \
             & {{ try {{ [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false) }} catch {{ }} }} 2>$null; \
             & {{ try {{ $OutputEncoding = [System.Text.UTF8Encoding]::new($false) }} catch {{ }} }} 2>$null; \
             {cmd}"
        );
        #[cfg(target_os = "windows")]
        let output = {
            let mut command = std::process::Command::new("powershell.exe");
            command
                .arg("-NoProfile")
                .arg("-Command")
                .arg(&wrapped_cmd)
                .no_window();
            inject_managed_runtime_env_if_needed(
                &mut command,
                runtime_resolver,
                managed_runtime_enabled,
            );
            command
                .output()
                .with_context(|| format!("failed to execute skill shell command: {cmd}"))?
        };
        #[cfg(not(target_os = "windows"))]
        let output = {
            let mut command = std::process::Command::new("bash");
            command.arg("-lc").arg(cmd);
            inject_managed_runtime_env_if_needed(
                &mut command,
                runtime_resolver,
                managed_runtime_enabled,
            );
            command
                .output()
                .with_context(|| format!("failed to execute skill shell command: {cmd}"))?
        };
        if !output.status.success() {
            anyhow::bail!(
                "Skill body shell command failed: {}",
                crate::storage::console_decode::decode_console_bytes(&output.stderr)
            );
        }
        result.push_str(&crate::storage::console_decode::decode_console_bytes(
            &output.stdout,
        ));
        rest = &after[end + 1..];
    }
    result.push_str(rest);
    Ok(result)
}

fn inject_managed_runtime_env_if_needed(
    command: &mut std::process::Command,
    runtime_resolver: Option<&dyn RuntimeResolver>,
    managed_runtime_enabled: bool,
) {
    if !managed_runtime_enabled {
        return;
    }
    let Some(runtime_resolver) = runtime_resolver else {
        return;
    };
    if let Ok(env) = ManagedRuntimeProcessEnv::from_resolver(runtime_resolver) {
        env.apply_to_command(command);
    }
}
