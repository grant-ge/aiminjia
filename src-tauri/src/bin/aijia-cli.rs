use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};
use app_lib::cli::{
    build_headless_driver, HeadlessAgentRequest, HeadlessBuildOptions, HeadlessRunOutput,
    HeadlessStreamEventSink,
};
use app_lib::runtime::ids::SessionId;
use app_lib::runtime::tools::permission::PermissionMode;
use serde_json::json;

const DEFAULT_MAX_ITERATIONS: usize = 120;

#[tokio::main]
async fn main() {
    match run().await {
        Ok(()) => {}
        Err(err) => {
            eprintln!("aijia error: {err:#}");
            std::process::exit(1);
        }
    }
}

async fn run() -> Result<()> {
    let args = parse_args(std::env::args().skip(1).collect())?;
    if args.help {
        print_usage();
        return Ok(());
    }
    if args.version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let output_format = args.output_format;
    let stream_output = output_format == OutputFormat::StreamJson;
    let stdout = Arc::new(Mutex::new(io::stdout()));
    let stream_event_sink: Option<HeadlessStreamEventSink> = if stream_output {
        let stdout = stdout.clone();
        Some(Arc::new(move |event| {
            emit_stdout_json_event(&stdout, &event);
        }))
    } else {
        None
    };

    let prompt = resolve_prompt(&args)?;
    if prompt.trim().is_empty() {
        anyhow::bail!(
            "Input must be provided via --prompt / --input-file / positional arg or stdin"
        );
    }

    let start = Instant::now();
    let driver = match build_headless_driver(HeadlessBuildOptions {
        add_dirs: args.add_dirs.clone(),
        session_id: args.session_id.map(SessionId::new),
        continue_latest: args.continue_latest,
        system_prompt: args.system_prompt.clone(),
        max_turns: args.max_turns,
        permission_mode: args.permission_mode,
        model: args.model.clone(),
        verbose: args.verbose,
        stream_event_sink,
    })
    .await
    {
        Ok(driver) => driver,
        Err(err) => {
            if matches!(output_format, OutputFormat::Json | OutputFormat::StreamJson) {
                let mut output = HeadlessRunOutput::default();
                mark_output_failed(&mut output, &err);
                output.duration_ms = start.elapsed().as_millis() as u64;
                write_output(&output, output_format, &stdout)?;
                std::process::exit(1);
            }
            anyhow::bail!(err);
        }
    };

    if stream_output {
        let init_output = driver.collect_output();
        emit_stdout_json_event(&stdout, &system_init_message(&init_output));
    }

    let output = match driver
        .run_agent_prompt(HeadlessAgentRequest { prompt })
        .await
    {
        Ok(output) => output,
        Err(err) => {
            let mut output = driver.collect_output();
            mark_output_failed(&mut output, &err);
            output.duration_ms = start.elapsed().as_millis() as u64;
            write_output(&output, output_format, &stdout)?;
            std::process::exit(1);
        }
    };

    write_output(&output, output_format, &stdout)?;
    if output.is_error {
        std::process::exit(1);
    }
    Ok(())
}

fn emit_stdout_json_event(stdout: &Arc<Mutex<io::Stdout>>, payload: &serde_json::Value) {
    let line = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());
    if let Ok(mut out) = stdout.lock() {
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
}

fn mark_output_failed(output: &mut HeadlessRunOutput, err: &anyhow::Error) {
    output.is_error = true;
    output.error_subtype = Some(classify_error_subtype(err.to_string()));
    output.execution_error = Some(err.to_string());

    if output.answer.trim().is_empty() {
        output.answer = output
            .execution_error
            .clone()
            .unwrap_or_else(|| "execution failure".to_string());
    }
}

fn classify_error_subtype(message: String) -> String {
    let message = message.to_lowercase();
    if message.contains("max-iterations") || message.contains("max iterations") {
        "error_max_turns".to_string()
    } else if message.contains("filtered") || message.contains("content filtered") {
        "error_content_filtered".to_string()
    } else {
        "error_during_execution".to_string()
    }
}

fn resolve_prompt(args: &CliArgs) -> Result<String> {
    if let Some(prompt) = args.print_prompt.as_ref() {
        return Ok(prompt.clone());
    }
    if let Some(prompt) = args.prompt.as_ref() {
        return Ok(prompt.clone());
    }
    if let Some(path) = args.input_file.as_ref() {
        return std::fs::read_to_string(path)
            .with_context(|| format!("failed to read input file {}", path.display()));
    }
    let mut stdin = io::stdin();
    if !stdin.is_terminal() {
        let mut prompt = String::new();
        stdin
            .read_to_string(&mut prompt)
            .context("failed to read prompt from stdin")?;
        return Ok(prompt);
    }
    Ok(String::new())
}

fn write_output(
    output: &HeadlessRunOutput,
    format: OutputFormat,
    stdout: &Arc<Mutex<io::Stdout>>,
) -> Result<()> {
    match format {
        OutputFormat::Text => {
            println!("{}", output.answer);
        }
        OutputFormat::Json => {
            let result = sdk_result_message(output);
            let line = serde_json::to_string(&result)?;
            if let Ok(mut out) = stdout.lock() {
                writeln!(out, "{line}")?;
                out.flush()?;
            } else {
                eprintln!("aijia error: failed to write result JSON to stdout");
            }
        }
        OutputFormat::StreamJson => {
            emit_stdout_json_event(stdout, &sdk_result_message(output));
        }
    }
    Ok(())
}

fn sdk_result_message(output: &HeadlessRunOutput) -> serde_json::Value {
    let subtype = output
        .error_subtype
        .as_deref()
        .unwrap_or("success")
        .to_string();
    let model = if output.execution_model.trim().is_empty() {
        output.model.clone()
    } else {
        output.execution_model.clone()
    };
    let requested_model = if output.requested_model.trim().is_empty() {
        model.clone()
    } else {
        output.requested_model.clone()
    };
    let mut result = json!({
        "answer": output.answer,
        "result": output.answer,
        "tool_calls": output.tool_calls,
        "iterations": output.iterations,
        "tokens": sdk_usage(output),
        "type": "result",
        "subtype": subtype,
        "is_error": output.is_error,
        "duration_ms": output.duration_ms,
        "duration_api_ms": output.duration_api_ms,
        "num_turns": output.iterations,
        "stop_reason": output.stop_reason.clone(),
        "session_id": output.session_id.clone(),
        "model": model,
        "requestedModel": requested_model,
        "executionModel": output.execution_model.clone(),
        "total_cost_usd": output.total_cost_usd,
        "usage": sdk_usage(output),
        "modelUsage": sdk_model_usage(output),
        "permission_denials": output.permission_denials.clone(),
        "uuid": output.uuid.clone(),
    });
    if !output.is_error {
        result["result"] = serde_json::Value::String(output.answer.clone());
    } else {
        let errors = output
            .execution_error
            .as_deref()
            .or(output.error_subtype.as_deref())
            .unwrap_or("execution failure");
        result["errors"] = json!([{ "type": "execution", "message": errors }]);
        result["error"] = serde_json::Value::String(errors.to_string());
    }
    result
}

fn sdk_usage(output: &HeadlessRunOutput) -> serde_json::Value {
    json!({
        "input_tokens": output.tokens.input,
        "output_tokens": output.tokens.output,
        "cache_creation_input_tokens": output.tokens.cache_creation_input,
        "cache_read_input_tokens": output.tokens.cache_read_input,
    })
}

fn sdk_model_usage(output: &HeadlessRunOutput) -> serde_json::Value {
    let model = if output.execution_model.trim().is_empty() {
        output.model.clone()
    } else {
        output.execution_model.clone()
    }
    .trim()
    .to_string();

    if model.is_empty() {
        return json!({});
    }
    let mut map = serde_json::Map::new();
    map.insert(
        model.clone(),
        json!({
            "inputTokens": output.tokens.input,
            "outputTokens": output.tokens.output,
            "cacheReadInputTokens": output.tokens.cache_read_input,
            "cacheCreationInputTokens": output.tokens.cache_creation_input,
            "webSearchRequests": 0,
            "costUSD": output.total_cost_usd,
            "contextWindow": 0,
            "maxOutputTokens": 0,
        }),
    );
    serde_json::Value::Object(map)
}

fn system_init_message(output: &HeadlessRunOutput) -> serde_json::Value {
    let requested_model = if output.requested_model.trim().is_empty() {
        output.model.clone()
    } else {
        output.requested_model.clone()
    };
    let execution_model = if output.execution_model.trim().is_empty() {
        output.model.clone()
    } else {
        output.execution_model.clone()
    };
    json!({
        "type": "system",
        "subtype": "init",
        "cwd": output.workspace.clone(),
        "session_id": output.session_id.clone(),
        "tools": output.tools.clone(),
        "mcp_servers": [],
        "model": execution_model,
        "requestedModel": requested_model,
        "executionModel": execution_model,
        "permissionMode": output.permission_mode.clone(),
        "slash_commands": [],
        "apiKeySource": "none",
        "betas": [],
        "claude_code_version": env!("CARGO_PKG_VERSION"),
        "output_style": "default",
        "agents": [],
        "skills": [],
        "plugins": [],
        "uuid": uuid::Uuid::new_v4().to_string(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Text,
    Json,
    StreamJson,
}

#[derive(Default)]
struct CliArgs {
    prompt: Option<String>,
    input_file: Option<PathBuf>,
    print: bool,
    print_prompt: Option<String>,
    output_format: OutputFormat,
    add_dirs: Vec<PathBuf>,
    session_id: Option<String>,
    system_prompt: Option<String>,
    max_turns: Option<usize>,
    permission_mode: PermissionMode,
    model: Option<String>,
    verbose: bool,
    continue_latest: bool,
    help: bool,
    version: bool,
}

impl Default for OutputFormat {
    fn default() -> Self {
        Self::Text
    }
}

fn parse_args(raw: Vec<String>) -> Result<CliArgs> {
    let mut args = CliArgs {
        permission_mode: PermissionMode::AcceptEdits,
        max_turns: Some(DEFAULT_MAX_ITERATIONS),
        ..CliArgs::default()
    };
    let mut i = 0usize;
    if raw.first().map(String::as_str) == Some("agent") {
        i = 1;
    }
    while i < raw.len() {
        match raw[i].as_str() {
            "--help" | "-h" => args.help = true,
            "--version" | "-v" => args.version = true,
            "--json" => args.output_format = OutputFormat::Json,
            "--prompt" => {
                i += 1;
                args.prompt = Some(require_value(&raw, i, "--prompt")?.to_string());
            }
            "--input-file" => {
                i += 1;
                args.input_file = Some(PathBuf::from(require_value(&raw, i, "--input-file")?));
            }
            "--workspace" => {
                i += 1;
                args.add_dirs = vec![PathBuf::from(require_value(&raw, i, "--workspace")?)];
            }
            "-p" | "--print" => {
                args.print = true;
                if raw
                    .get(i + 1)
                    .map(|value| !looks_like_flag(value))
                    .unwrap_or(false)
                {
                    i += 1;
                    args.print_prompt = Some(raw[i].clone());
                }
            }
            "--output-format" => {
                i += 1;
                args.output_format =
                    parse_output_format(require_value(&raw, i, "--output-format")?)?;
            }
            "--add-dir" => {
                i += 1;
                let mut consumed = false;
                while i < raw.len() && !looks_like_flag(&raw[i]) {
                    args.add_dirs.push(PathBuf::from(&raw[i]));
                    consumed = true;
                    i += 1;
                }
                if !consumed {
                    anyhow::bail!("missing value for --add-dir");
                }
                i -= 1;
            }
            "--session-id" => {
                i += 1;
                args.session_id = Some(require_value(&raw, i, "--session-id")?.to_string());
            }
            "--system-prompt" => {
                i += 1;
                args.system_prompt = Some(require_value(&raw, i, "--system-prompt")?.to_string());
            }
            "--max-turns" => {
                i += 1;
                let value = require_value(&raw, i, "--max-turns")?;
                args.max_turns = Some(
                    value
                        .parse::<usize>()
                        .with_context(|| format!("invalid --max-turns value: {value}"))?,
                );
            }
            "--max-iterations" => {
                i += 1;
                let value = require_value(&raw, i, "--max-iterations")?;
                args.max_turns = Some(
                    value
                        .parse::<usize>()
                        .with_context(|| format!("invalid --max-iterations value: {value}"))?,
                );
            }
            "--permission-mode" => {
                i += 1;
                args.permission_mode =
                    parse_permission_mode(require_value(&raw, i, "--permission-mode")?)?;
            }
            "--model" => {
                i += 1;
                args.model = Some(require_value(&raw, i, "--model")?.to_string());
            }
            "--verbose" => args.verbose = true,
            "-c" | "--continue" => args.continue_latest = true,
            "agent" if i == 0 => {}
            value if looks_like_flag(value) => anyhow::bail!("unknown argument: {value}"),
            value => {
                if args.prompt.is_some() {
                    anyhow::bail!("unexpected extra positional argument: {value}");
                }
                args.prompt = Some(value.to_string());
            }
        }
        i += 1;
    }
    Ok(args)
}

fn looks_like_flag(value: &str) -> bool {
    value.starts_with('-') && value != "-"
}

fn require_value<'a>(raw: &'a [String], index: usize, flag: &str) -> Result<&'a str> {
    raw.get(index)
        .map(|s| s.as_str())
        .filter(|s| !looks_like_flag(s))
        .ok_or_else(|| anyhow::anyhow!("missing value for {flag}"))
}

fn parse_output_format(value: &str) -> Result<OutputFormat> {
    match value {
        "text" => Ok(OutputFormat::Text),
        "json" => Ok(OutputFormat::Json),
        "stream-json" => Ok(OutputFormat::StreamJson),
        _ => anyhow::bail!("invalid --output-format value: {value}"),
    }
}

fn parse_permission_mode(value: &str) -> Result<PermissionMode> {
    match value {
        "default" => Ok(PermissionMode::Default),
        "plan" => Ok(PermissionMode::Plan),
        "acceptEdits" => Ok(PermissionMode::AcceptEdits),
        "bypassPermissions" => Ok(PermissionMode::AcceptEdits),
        "dontAsk" => Ok(PermissionMode::DontAsk),
        "auto" => Ok(PermissionMode::AcceptEdits),
        _ => anyhow::bail!("invalid --permission-mode value: {value}"),
    }
}

fn print_usage() {
    eprintln!(
        "Usage: aijia agent --prompt <text> [--workspace <dir>] [--session-id <id>] [--system-prompt <text>] [--max-iterations <n>] [--json]\n       aijia -p <text> [--output-format text|json|stream-json] [--add-dir <dirs...>] [--model <model>] [--verbose] [-c, --continue]\n       aijia --version | -v"
    );
}
