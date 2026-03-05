//! Sandbox configuration — timeout, allowed paths, forbidden imports.
//!
//! Controls what Python code is allowed to do. Prevents dangerous operations
//! like running subprocesses or writing files outside the workspace.

use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// Sandbox configuration for Python execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxConfig {
    /// Maximum execution time in seconds.
    /// Desktop app: generous default (5 min) — just prevents infinite loops.
    /// User can always cancel via stop_streaming.
    pub timeout_seconds: u32,
    /// Maximum memory usage in MB (advisory — enforced via Python resource module).
    pub memory_limit_mb: u32,
    /// Directories the Python script is allowed to access.
    pub allowed_paths: Vec<PathBuf>,
    /// Maximum output size in bytes before truncation.
    pub max_output_bytes: usize,
    /// Python modules that are forbidden from being imported.
    pub forbidden_modules: Vec<String>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            timeout_seconds: 300,
            memory_limit_mb: 512,
            allowed_paths: Vec::new(),
            max_output_bytes: 1_000_000, // 1 MB
            forbidden_modules: vec![
                "subprocess".to_string(),
                "importlib".to_string(),
                // NOTE: ctypes removed — numpy needs it at runtime for C extension loading.
                // Still blocked in validate_code() for user-written code.
                "multiprocessing".to_string(),
                // NOTE: shutil, requests, socket, http, urllib removed —
                // local desktop app needs file ops and network access for data analysis.
                // Write-path restriction still protects against writes outside workspace.
            ],
        }
    }
}

/// Check if `pattern` appears as a standalone call (not a method call like `pd.eval()`).
///
/// Returns `true` if `pattern` is found and is NOT preceded by `.`, an alphanumeric
/// character, or `_`. This prevents false positives like `pd.eval(`, `re.compile(`,
/// `df.eval(` while still catching bare `eval(`, `exec(`, `compile(`.
fn contains_standalone(code: &str, pattern: &str) -> bool {
    for (i, _) in code.match_indices(pattern) {
        if i == 0 {
            return true;
        }
        let prev = code.as_bytes()[i - 1];
        if prev == b'.' || prev.is_ascii_alphanumeric() || prev == b'_' {
            continue;
        }
        return true;
    }
    false
}

impl SandboxConfig {
    /// Create a sandbox config for the given workspace path.
    /// The script will be allowed to access uploads/ and analysis/ subdirs.
    pub fn for_workspace(workspace: &PathBuf) -> Self {
        let mut config = Self::default();
        config.allowed_paths = vec![
            workspace.clone(),
            workspace.join("uploads"),
            workspace.join("exports"),
            workspace.join("reports"),
            workspace.join("charts"),
            workspace.join("analysis"),
            workspace.join("temp"),
        ];
        config
    }

    /// Validate Python code against sandbox rules.
    /// Returns Ok(()) if the code passes all checks, Err with reason otherwise.
    pub fn validate_code(&self, code: &str) -> Result<(), String> {
        // Check for forbidden imports
        for module in &self.forbidden_modules {
            // Check various import patterns
            let patterns = [
                format!("import {}", module),
                format!("from {} import", module),
                format!("__import__('{}')", module),
                format!("__import__(\"{}\")", module),
            ];
            for pattern in &patterns {
                if code.contains(pattern) {
                    return Err(format!(
                        "Forbidden module '{}': this module is not allowed in the sandbox",
                        module
                    ));
                }
            }
        }

        // Check for dangerous standalone built-in calls.
        // Uses contains_standalone() to avoid false positives:
        //   pd.eval(), df.eval(), re.compile() → allowed (method calls)
        //   eval(), exec(), compile()          → blocked (bare built-in calls)
        let standalone_calls = [
            ("exec(", "exec() is not allowed"),
            ("exec (", "exec() is not allowed"),
            ("eval(", "eval() is not allowed"),
            ("eval (", "eval() is not allowed"),
            ("compile(", "compile() is not allowed"),
            ("compile (", "compile() is not allowed"),
        ];
        for (pattern, msg) in &standalone_calls {
            if contains_standalone(code, pattern) {
                return Err(msg.to_string());
            }
        }

        // Check for dangerous calls (substring match — these have unique prefixes)
        let dangerous_calls = [
            ("os.system", "os.system() is not allowed"),
            ("os.popen", "os.popen() is not allowed"),
            ("os.exec", "os.exec*() is not allowed"),
            ("os.fork", "os.fork() is not allowed"),
            ("os.spawn", "os.spawn*() is not allowed"),
            ("os.posix_spawn", "os.posix_spawn() is not allowed"),
            ("builtins.__import__", "builtins.__import__() is not allowed"),
            ("_real_import", "accessing _real_import is not allowed"),
            ("_safe_import._real", "accessing _safe_import._real is not allowed"),
            ("import ctypes", "ctypes is not allowed"),
            ("from ctypes", "ctypes is not allowed"),
            ("_original_open", "accessing _original_open is not allowed"),
            ("_safe_open._orig", "bypassing _safe_open is not allowed"),
        ];
        for (pattern, msg) in &dangerous_calls {
            if code.contains(pattern) {
                return Err(msg.to_string());
            }
        }

        Ok(())
    }

    /// Generate Python preamble that sets up resource limits and pre-loaded utilities.
    /// This code is prepended to user code before execution.
    ///
    /// **Execution order matters for correctness:**
    /// 1. Basic setup (sys, os, warnings, encoding, paths, working directory)
    /// 2. Trusted package imports (pandas, numpy, scipy) — these need unrestricted
    ///    access to internal modules like ctypes, importlib.machinery, etc.
    /// 3. File write restriction — blocks writes outside workspace
    /// 4. Utility functions (_smart_read_csv, _load_data, etc.)
    ///
    /// Security model (local desktop app):
    /// - **Static validation** (`validate_code`): blocks dangerous patterns before execution
    /// - **File write restriction** (`_safe_open`): confines writes to workspace directories
    /// - **Resource limits**: 30s timeout + 512MB memory cap
    /// - No runtime import hook — static validation is sufficient for a local app,
    ///   and the hook caused repeated breakage with legitimate library dependencies
    ///   (openpyxl→shutil, urllib.parse, etc.)
    pub fn preamble(&self) -> String {
        // Part 1: Basic setup (dynamic — uses Rust format! for allowed_paths)
        let basic_setup = format!(
            r#"import sys
import os
import builtins
import warnings
warnings.filterwarnings('ignore')

# Force UTF-8 encoding for stdout/stderr (prevents GBK issues on Windows)
if hasattr(sys.stdout, 'reconfigure'):
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')
if hasattr(sys.stderr, 'reconfigure'):
    sys.stderr.reconfigure(encoding='utf-8', errors='replace')

# Restrict file access to allowed directories
_ALLOWED_PATHS = {allowed_paths}

# Set recursion limit
sys.setrecursionlimit(2000)

# Enforce memory limit (Unix only, advisory on macOS, hard on Linux)
try:
    import resource as _resource
    _mem_bytes = {memory_limit_mb} * 1024 * 1024
    _resource.setrlimit(_resource.RLIMIT_AS, (_mem_bytes, _mem_bytes))
    del _resource
except (ImportError, ValueError, OSError):
    pass  # Windows or unsupported platform

# Working directory (workspace root — first element of _ALLOWED_PATHS)
if _ALLOWED_PATHS:
    try:
        os.chdir(_ALLOWED_PATHS[0])
    except OSError:
        pass
"#,
            allowed_paths = format!(
                "[{}]",
                self.allowed_paths
                    .iter()
                    .map(|p| {
                        let s = p.display().to_string();
                        // Escape backslashes and single quotes to prevent Python injection
                        let escaped = s.replace('\\', "\\\\").replace('\'', "\\'");
                        format!("'{}'", escaped)
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            memory_limit_mb = self.memory_limit_mb,
        );

        // Part 2: Pre-loaded imports (performance — avoids cold-start lag per execution)
        let trusted_imports = TRUSTED_IMPORTS;

        // Part 3: File write restriction (no runtime import hook — static validation is enough)
        let file_write_hook = r#"
# ── File write restriction (path enforcement layer) ──
# Wraps builtins.open to block writes outside _ALLOWED_PATHS.
# Read operations are unrestricted (pandas/numpy read from site-packages, /tmp, etc.).
# Also tracks all write paths in _written_files for file lifecycle management.
if '_written_files' not in dir():
    _written_files = []
_original_open = builtins.open
def _safe_open(file, mode='r', *args, **kwargs):
    if isinstance(file, (str, bytes)):
        file_str = file if isinstance(file, str) else file.decode('utf-8', errors='replace')
        # Check write operations: 'w', 'a', 'x' (create)
        if any(m in str(mode) for m in ('w', 'a', 'x')):
            try:
                # Resolve to absolute path for comparison
                abs_path = os.path.realpath(os.path.abspath(file_str))
                allowed = any(
                    abs_path.startswith(os.path.realpath(p))
                    for p in _ALLOWED_PATHS
                ) if _ALLOWED_PATHS else False
                if not allowed:
                    raise PermissionError(
                        f"Writing to '{file_str}' is blocked (outside workspace). "
                        f"Allowed directories: {', '.join(_ALLOWED_PATHS)}"
                    )
                # Track write path for file lifecycle management
                _wf = globals().get('_written_files') or (getattr(builtins, '_written_files', None))
                if _wf is not None and isinstance(_wf, list):
                    _wf.append(abs_path)
            except (TypeError, ValueError):
                pass  # Non-path argument (e.g. file descriptor) — let through
    return _original_open(file, mode, *args, **kwargs)
builtins.open = _safe_open
"#;

        // Part 4: Utility functions (static — no Rust format! needed)
        let utilities = UTILITY_FUNCTIONS;

        format!("{}\n{}\n{}\n{}", basic_setup, trusted_imports, file_write_hook, utilities)
    }
}

/// Pre-loaded package imports (saves ~2-3s cold start per execution).
///
/// Static string (not processed by `format!`) so Python f-strings with braces work.
const TRUSTED_IMPORTS: &str = r###"
# ============================================================
# Pre-loaded imports (saves ~2-3s cold start per execution)
# ============================================================
import json
import glob
import pandas as pd
import numpy as np
try:
    import openpyxl
except ImportError:
    pass
try:
    from scipy import stats as scipy_stats
except ImportError:
    scipy_stats = None
"###;

/// Utility functions injected into the Python preamble.
///
/// Uses already-imported modules (pandas, numpy, os, json, glob, openpyxl).
/// Static string (not processed by `format!`) so Python f-strings work.
const UTILITY_FUNCTIONS: &str = r###"
# ============================================================
# Utility functions — encoding detection, file I/O, formatting
# ============================================================

def _smart_read_csv(path, **kwargs):
    """Read CSV with encoding auto-detection (UTF-8 -> GBK -> latin-1)."""
    for enc in ['utf-8', 'utf-8-sig', 'gbk', 'gb2312', 'gb18030', 'latin-1']:
        try:
            return pd.read_csv(path, encoding=enc, **kwargs)
        except (UnicodeDecodeError, UnicodeError):
            continue
    return pd.read_csv(path, encoding='latin-1', errors='replace', **kwargs)

def _smart_read_data(path, **kwargs):
    """Read CSV, Excel, JSON, or Parquet with encoding auto-detection."""
    lower = path.lower()
    if lower.endswith('.csv') or lower.endswith('.tsv'):
        return _smart_read_csv(path, **kwargs)
    elif lower.endswith('.json') or lower.endswith('.jsonl'):
        try:
            return pd.read_json(path, **kwargs)
        except ValueError:
            return pd.read_json(path, lines=True, **kwargs)
    elif lower.endswith('.parquet'):
        return pd.read_parquet(path, **kwargs)
    else:
        # Use openpyxl read_only + data_only for fast streaming reads,
        # avoiding formula evaluation and style parsing that cause timeouts.
        try:
            from openpyxl import load_workbook
            nrows = kwargs.pop('nrows', None)
            max_row = (nrows + 1) if nrows else None
            wb = load_workbook(path, read_only=True, data_only=True)
            ws = wb.active
            rows = list(ws.iter_rows(max_row=max_row, values_only=True))
            wb.close()
            if rows:
                header = [str(c).strip() if c is not None else f'col_{i}' for i, c in enumerate(rows[0])]
                df = pd.DataFrame(rows[1:], columns=header)
            else:
                df = pd.DataFrame()
        except Exception:
            # Fallback to pd.read_excel if openpyxl streaming fails
            df = pd.read_excel(path, **kwargs)
        # Fix garbled column names from GBK-encoded Excel files
        df.columns = [str(c).strip() for c in df.columns]
        return df

def _smart_write_csv(df, path, **kwargs):
    """Write CSV with UTF-8-BOM encoding (opens correctly in Excel on Windows)."""
    kwargs.setdefault('encoding', 'utf-8-sig')
    kwargs.setdefault('index', False)
    df.to_csv(path, **kwargs)

def _find_data_file(pattern='uploads/*'):
    """Find the first data file matching the pattern."""
    files = glob.glob(pattern)
    data_exts = ('.xlsx', '.xls', '.csv', '.tsv', '.json', '.jsonl', '.parquet')
    data_files = [f for f in files if any(f.lower().endswith(e) for e in data_exts)]
    return data_files[0] if data_files else (files[0] if files else None)

def _load_data(path=None):
    """Load a data file into a DataFrame. Auto-detects if path is None."""
    if path is None:
        path = _find_data_file()
        if path is None:
            raise FileNotFoundError("No data files found in uploads/")
    return _smart_read_data(path)

def _print_table(headers, rows, title=''):
    """Print a formatted markdown table."""
    if title:
        print(f"\n**{title}**")
    if not rows:
        return
    col_widths = [max(len(str(h)), max((len(str(r[i])) for r in rows), default=0)) for i, h in enumerate(headers)]
    header_line = '| ' + ' | '.join(str(h).ljust(w) for h, w in zip(headers, col_widths)) + ' |'
    sep_line = '|' + '|'.join('-' * (w + 2) for w in col_widths) + '|'
    print(header_line)
    print(sep_line)
    for row in rows:
        print('| ' + ' | '.join(str(row[i] if i < len(row) else '').ljust(w) for i, w in enumerate(col_widths)) + ' |')

def _export_detail(df, filename, title='明细数据', preview_rows=15, format='excel'):
    """Export a DataFrame and print an inline preview table.

    Saves the full data to exports/<filename>.<ext> and prints the first
    `preview_rows` rows as a Markdown table, followed by a download hint.

    Supported formats:
        - 'excel' (default): .xlsx via openpyxl
        - 'csv': .csv with UTF-8-BOM encoding (Excel-compatible)
        - 'json': .json with orient='records'

    Args:
        df: DataFrame to export
        filename: Base filename (with or without extension), e.g. 'step1_exclusion_detail'
        title: Section title for the inline preview
        preview_rows: Number of rows to show inline (default 15)
        format: Output format — 'excel', 'csv', or 'json' (default 'excel')

    Returns:
        The full path of the exported file.
    """
    import os, json as _json_mod
    export_dir = 'exports'
    os.makedirs(export_dir, exist_ok=True)
    # Sanitize: prevent path traversal (e.g. "../../etc/passwd")
    base = os.path.basename(filename)
    # Strip known extensions to prevent double extension
    for ext in ('.xlsx', '.xls', '.csv', '.json', '.tsv'):
        if base.lower().endswith(ext):
            base = base[:-len(ext)]
            break

    # Determine extension and write method by format
    fmt = format.lower().strip()
    if fmt == 'csv':
        out_name = f'{base}.csv'
        fmt_label = 'CSV'
    elif fmt == 'json':
        out_name = f'{base}.json'
        fmt_label = 'JSON'
    else:
        fmt = 'excel'
        out_name = f'{base}.xlsx'
        fmt_label = 'Excel'
    full_path = os.path.join(export_dir, out_name)

    # Write file with error handling
    try:
        if fmt == 'csv':
            df.to_csv(full_path, index=False, encoding='utf-8-sig')
        elif fmt == 'json':
            df.to_json(full_path, orient='records', force_ascii=False, indent=2)
        else:
            df.to_excel(full_path, index=False, engine='openpyxl')
    except Exception as _e:
        print(f'文件写入失败: {_e}', file=__import__("sys").stderr)
        return None

    # Verify file was actually created with content
    if not os.path.exists(full_path) or os.path.getsize(full_path) == 0:
        print(f'文件写入验证失败: {full_path}', file=__import__("sys").stderr)
        return None

    n = len(df)
    # Emit structured marker for auto-registration in DB (only after verified write)
    # Use json.dumps to prevent title/filename injection into the JSON structure
    _marker = _json_mod.dumps({"path": full_path, "filename": out_name, "title": title, "format": fmt, "rows": n}, ensure_ascii=False)
    print(f'__GENERATED_FILE__:{_marker}')
    print(f'\n## {title}（共 {n} 条）')
    # Inline preview
    preview = df.head(preview_rows)
    headers = list(preview.columns)
    rows = []
    for _, row in preview.iterrows():
        rows.append([str(v) for v in row.values])
    _print_table(headers, rows)
    if n > preview_rows:
        print(f'\n> 完整 {n} 条明细已导出到 {fmt_label}')
    else:
        print(f'\n> 完整明细已导出到 {fmt_label}')
    return full_path

# ============================================================
# Workspace file management utilities
# ============================================================

def _ws_list(path='.', pattern='*', recursive=False):
    """List files in workspace directory with size and modification time.

    Args:
        path: Relative path from workspace root (default '.', can be 'uploads', 'exports', etc.)
        pattern: Glob pattern (default '*', e.g. '*.xlsx', '*.csv')
        recursive: If True, search subdirectories recursively

    Returns:
        DataFrame with columns: name, path, size_kb, modified, type
    """
    import os, glob as _glob, time as _time
    base = os.path.abspath(path)
    if recursive:
        search = os.path.join(base, '**', pattern)
        files = _glob.glob(search, recursive=True)
    else:
        search = os.path.join(base, pattern)
        files = _glob.glob(search)

    results = []
    for f in sorted(files):
        if os.path.isfile(f):
            stat = os.stat(f)
            results.append({
                'name': os.path.basename(f),
                'path': os.path.relpath(f),
                'size_kb': round(stat.st_size / 1024, 1),
                'modified': _time.strftime('%Y-%m-%d %H:%M', _time.localtime(stat.st_mtime)),
                'type': os.path.splitext(f)[1].lstrip('.')
            })
    df = pd.DataFrame(results)
    if len(df) > 0:
        _print_table(list(df.columns), [list(r) for _, r in df.iterrows()], f'文件列表 ({path})')
    else:
        print(f'目录 {path} 中没有匹配 {pattern} 的文件')
    return df

def _ws_search(keyword, path='.', extensions=None):
    """Search file contents for a keyword.

    Args:
        keyword: Text to search for (case-insensitive)
        path: Directory to search in
        extensions: List of extensions to search (default: common text/data files)

    Returns:
        List of dicts with file, line_number, line_content
    """
    import os, glob as _glob
    if extensions is None:
        extensions = ['.csv', '.txt', '.json', '.md', '.py', '.log', '.tsv']

    results = []
    for ext in extensions:
        for f in _glob.glob(os.path.join(path, '**', f'*{ext}'), recursive=True):
            try:
                with open(f, 'r', encoding='utf-8', errors='ignore') as fh:
                    for i, line in enumerate(fh, 1):
                        if keyword.lower() in line.lower():
                            results.append({
                                'file': os.path.relpath(f),
                                'line': i,
                                'content': line.strip()[:200]
                            })
                            if len(results) >= 50:
                                break
            except Exception:
                continue
            if len(results) >= 50:
                break
        if len(results) >= 50:
            break

    if results:
        print(f'找到 {len(results)} 处匹配 "{keyword}"：')
        for r in results[:20]:
            print(f'  {r["file"]}:{r["line"]} — {r["content"][:100]}')
        if len(results) > 20:
            print(f'  ... 还有 {len(results)-20} 处匹配')
    else:
        print(f'未找到包含 "{keyword}" 的文件')
    return results

def _ws_info(path):
    """Get detailed info about a file or directory.

    Returns:
        Dict with size, type, modified, preview (first 5 lines for text files).
    """
    import os, time as _time
    abs_path = os.path.abspath(path)
    if not os.path.exists(abs_path):
        print(f'文件不存在: {path}')
        return None

    stat = os.stat(abs_path)
    info = {
        'path': os.path.relpath(abs_path),
        'size': f'{stat.st_size / 1024:.1f} KB' if stat.st_size < 1024*1024 else f'{stat.st_size / (1024*1024):.1f} MB',
        'modified': _time.strftime('%Y-%m-%d %H:%M:%S', _time.localtime(stat.st_mtime)),
        'type': 'directory' if os.path.isdir(abs_path) else os.path.splitext(abs_path)[1].lstrip('.'),
    }

    if os.path.isdir(abs_path):
        items = os.listdir(abs_path)
        info['items'] = len(items)
        info['contents'] = items[:20]
    elif os.path.isfile(abs_path) and stat.st_size < 1024 * 1024:
        ext = os.path.splitext(abs_path)[1].lower()
        if ext in ('.csv', '.txt', '.json', '.md', '.py', '.log', '.tsv', '.html'):
            try:
                with open(abs_path, 'r', encoding='utf-8', errors='ignore') as f:
                    info['preview'] = [f.readline().rstrip() for _ in range(5)]
            except Exception:
                pass

    for k, v in info.items():
        print(f'  {k}: {v}')
    return info

def _ws_convert(input_path, output_format='csv'):
    """Convert a data file between formats (csv/excel/json/parquet).

    Args:
        input_path: Path to source file
        output_format: Target format ('csv', 'excel', 'json', 'parquet')

    Returns:
        Path to output file
    """
    import os
    df = _smart_read_data(input_path)
    base = os.path.splitext(os.path.basename(input_path))[0]
    out_dir = 'exports'
    os.makedirs(out_dir, exist_ok=True)

    fmt = output_format.lower().strip()
    if fmt == 'csv':
        out = os.path.join(out_dir, f'{base}.csv')
        _smart_write_csv(df, out)
    elif fmt in ('excel', 'xlsx'):
        out = os.path.join(out_dir, f'{base}.xlsx')
        df.to_excel(out, index=False, engine='openpyxl')
    elif fmt == 'json':
        out = os.path.join(out_dir, f'{base}.json')
        df.to_json(out, orient='records', force_ascii=False, indent=2)
    elif fmt == 'parquet':
        out = os.path.join(out_dir, f'{base}.parquet')
        df.to_parquet(out, index=False)
    else:
        raise ValueError(f'Unsupported format: {output_format}')

    print(f'已转换: {input_path} → {out} ({len(df)} 行)')
    return out

def _ws_merge(file_paths, output_name='merged', output_format='excel'):
    """Merge multiple data files into one.

    Args:
        file_paths: List of file paths to merge
        output_name: Base filename for output (default 'merged')
        output_format: Output format ('excel', 'csv', 'json')

    Returns:
        Merged DataFrame
    """
    dfs = []
    for fp in file_paths:
        try:
            df = _smart_read_data(fp)
            df['_source_file'] = os.path.basename(fp)
            dfs.append(df)
            print(f'  读取 {os.path.basename(fp)}: {len(df)} 行, {len(df.columns)} 列')
        except Exception as e:
            print(f'  跳过 {fp}: {e}')

    if not dfs:
        print('没有成功读取任何文件')
        return pd.DataFrame()

    merged = pd.concat(dfs, ignore_index=True)
    print(f'\n合并结果: {len(merged)} 行, {len(merged.columns)} 列')
    return _export_detail(merged, output_name, f'合并数据（{len(file_paths)} 个文件）', format=output_format)
"###;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SandboxConfig::default();
        assert_eq!(config.timeout_seconds, 300);
        assert_eq!(config.memory_limit_mb, 512);
        assert!(config.forbidden_modules.contains(&"subprocess".to_string()));
        assert!(config.forbidden_modules.contains(&"importlib".to_string()));
        assert!(config.forbidden_modules.contains(&"multiprocessing".to_string()));
        // Relaxed for local desktop use — no longer forbidden
        assert!(!config.forbidden_modules.contains(&"shutil".to_string()));
        assert!(!config.forbidden_modules.contains(&"requests".to_string()));
        assert!(!config.forbidden_modules.contains(&"socket".to_string()));
        assert!(!config.forbidden_modules.contains(&"http".to_string()));
        assert!(!config.forbidden_modules.contains(&"urllib".to_string()));
        // ctypes removed from forbidden_modules (numpy needs it at runtime)
        assert!(!config.forbidden_modules.contains(&"ctypes".to_string()));
    }

    #[test]
    fn test_validate_code_ok() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("import pandas as pd\nprint('hello')").is_ok());
    }

    #[test]
    fn test_validate_code_forbidden_import() {
        let config = SandboxConfig::default();
        let result = config.validate_code("import subprocess");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("subprocess"));
    }

    #[test]
    fn test_validate_code_from_import() {
        let config = SandboxConfig::default();
        // socket is no longer forbidden (relaxed for local desktop use)
        assert!(config.validate_code("from socket import socket").is_ok());
        // subprocess is still forbidden
        assert!(config.validate_code("from subprocess import run").is_err());
    }

    #[test]
    fn test_validate_code_exec_blocked() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("exec('print(1)')").is_err());
        // Also block exec with space before paren
        assert!(config.validate_code("exec ('print(1)')").is_err());
    }

    #[test]
    fn test_validate_code_os_fork_blocked() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("os.fork()").is_err());
    }

    #[test]
    fn test_validate_code_importlib_blocked() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("import importlib").is_err());
    }

    #[test]
    fn test_validate_code_ctypes_blocked() {
        let config = SandboxConfig::default();
        // ctypes is not in forbidden_modules (numpy needs it at runtime)
        // but is blocked by validate_code() via dangerous_calls
        assert!(config.validate_code("import ctypes").is_err());
        assert!(config.validate_code("from ctypes import cdll").is_err());
    }

    #[test]
    fn test_validate_code_builtins_import_blocked() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("builtins.__import__('subprocess')").is_err());
    }

    #[test]
    fn test_validate_code_real_import_blocked() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("_real_import('subprocess')").is_err());
        assert!(config.validate_code("_safe_import._real('subprocess')").is_err());
    }

    #[test]
    fn test_validate_code_original_open_blocked() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("_original_open('/etc/passwd', 'w')").is_err());
        assert!(config.validate_code("f = _original_open('/tmp/x')").is_err());
    }

    #[test]
    fn test_preamble_no_import_hook() {
        // Runtime import hook was removed — static validation is sufficient for a local app.
        // Verify the hook is NOT present (it caused repeated breakage with library dependencies).
        let config = SandboxConfig::default();
        let preamble = config.preamble();
        assert!(!preamble.contains("builtins.__import__ = _safe_import"),
            "Runtime import hook should NOT be in preamble (removed for stability)");
        assert!(!preamble.contains("_FORBIDDEN_MODULES"),
            "Runtime _FORBIDDEN_MODULES should NOT be in preamble");
    }

    #[test]
    fn test_for_workspace() {
        let workspace = PathBuf::from("/tmp/workspace");
        let config = SandboxConfig::for_workspace(&workspace);
        assert_eq!(config.allowed_paths.len(), 7);
        assert_eq!(config.allowed_paths[0], workspace);
        assert!(config.allowed_paths[1].ends_with("uploads"));
    }

    #[test]
    fn test_preamble_contains_paths() {
        let mut config = SandboxConfig::default();
        config.allowed_paths = vec![PathBuf::from("/tmp/test")];
        let preamble = config.preamble();
        assert!(preamble.contains("/tmp/test"));
    }

    // -- eval() and compile() blocking -----------------------------------------

    #[test]
    fn test_validate_code_eval_blocked() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("result = eval('1+2')").is_err());
    }

    #[test]
    fn test_validate_code_compile_blocked() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("code = compile('print(1)', '<string>', 'exec')").is_err());
    }

    // -- os.system and os.popen blocking ---------------------------------------

    #[test]
    fn test_validate_code_os_system_blocked() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("os.system('ls')").is_err());
    }

    #[test]
    fn test_validate_code_os_popen_blocked() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("os.popen('cat /etc/passwd')").is_err());
    }

    #[test]
    fn test_validate_code_os_exec_blocked() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("os.execv('/bin/sh', [])").is_err());
    }

    // -- __import__() with quotes blocking -------------------------------------

    #[test]
    fn test_validate_code_dunder_import_single_quotes() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("mod = __import__('subprocess')").is_err());
    }

    #[test]
    fn test_validate_code_dunder_import_double_quotes() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("mod = __import__(\"subprocess\")").is_err());
    }

    // -- All forbidden modules are blocked -------------------------------------

    #[test]
    fn test_all_forbidden_modules_blocked() {
        let config = SandboxConfig::default();
        for module in &config.forbidden_modules {
            let code = format!("import {}", module);
            assert!(
                config.validate_code(&code).is_err(),
                "Module '{}' should be blocked via 'import {}'",
                module,
                module
            );

            let code2 = format!("from {} import something", module);
            assert!(
                config.validate_code(&code2).is_err(),
                "Module '{}' should be blocked via 'from {} import'",
                module,
                module
            );
        }
    }

    // -- Valid code should pass -------------------------------------------------

    #[test]
    fn test_validate_code_complex_valid() {
        let config = SandboxConfig::default();
        let code = r#"
import pandas as pd
import numpy as np
from scipy import stats

df = pd.read_csv('data.csv')
mean = df['salary'].mean()
std = df['salary'].std()
z_scores = stats.zscore(df['salary'])
print(f"Mean: {mean}, Std: {std}")
"#;
        assert!(config.validate_code(code).is_ok());
    }

    #[test]
    fn test_validate_code_multiline_with_safe_imports() {
        let config = SandboxConfig::default();
        let code = "import os\nimport json\nimport pandas\nprint(os.getcwd())";
        assert!(config.validate_code(code).is_ok());
    }

    // -- Preamble content checks -----------------------------------------------

    #[test]
    fn test_preamble_contains_safe_open() {
        let config = SandboxConfig::default();
        let preamble = config.preamble();
        assert!(
            preamble.contains("_safe_open"),
            "Preamble should define _safe_open wrapper"
        );
        assert!(
            preamble.contains("_original_open"),
            "Preamble should save original open as _original_open"
        );
        assert!(
            preamble.contains("builtins.open = _safe_open"),
            "Preamble should replace builtins.open with _safe_open"
        );
    }

    #[test]
    fn test_preamble_safe_open_checks_write_modes() {
        let config = SandboxConfig::default();
        let preamble = config.preamble();
        // Write mode characters that should be checked
        for mode_char in &["'w'", "'a'", "'x'"] {
            assert!(
                preamble.contains(mode_char),
                "Safe open should check write mode '{}'",
                mode_char
            );
        }
    }

    #[test]
    fn test_preamble_contains_utf8_setup() {
        let config = SandboxConfig::default();
        let preamble = config.preamble();
        assert!(preamble.contains("utf-8"), "Preamble should configure UTF-8 encoding");
        assert!(preamble.contains("reconfigure"), "Preamble should reconfigure stdout");
    }

    #[test]
    fn test_preamble_contains_smart_read_functions() {
        let config = SandboxConfig::default();
        let preamble = config.preamble();
        assert!(preamble.contains("_smart_read_csv"), "Preamble should define _smart_read_csv");
        assert!(preamble.contains("_smart_read_data"), "Preamble should define _smart_read_data");
    }

    #[test]
    fn test_preamble_multiple_allowed_paths() {
        let mut config = SandboxConfig::default();
        config.allowed_paths = vec![
            PathBuf::from("/workspace/uploads"),
            PathBuf::from("/workspace/analysis"),
            PathBuf::from("/workspace/temp"),
        ];
        let preamble = config.preamble();
        assert!(preamble.contains("/workspace/uploads"));
        assert!(preamble.contains("/workspace/analysis"));
        assert!(preamble.contains("/workspace/temp"));
    }

    #[test]
    fn test_preamble_empty_allowed_paths() {
        let config = SandboxConfig::default();
        let preamble = config.preamble();
        assert!(preamble.contains("_ALLOWED_PATHS = []"));
    }

    // -- Pre-loaded imports ----------------------------------------------------

    #[test]
    fn test_preamble_preloads_packages() {
        let config = SandboxConfig::default();
        let preamble = config.preamble();
        assert!(preamble.contains("import pandas as pd"), "Preamble should pre-load pandas");
        assert!(preamble.contains("import numpy as np"), "Preamble should pre-load numpy");
        assert!(preamble.contains("import openpyxl"), "Preamble should pre-load openpyxl");
    }

    // -- Standalone call detection (no false positives) -------------------------

    #[test]
    fn test_validate_code_pd_eval_allowed() {
        let config = SandboxConfig::default();
        // pd.eval() and df.eval() are method calls, not bare eval()
        assert!(config.validate_code("result = pd.eval('A + B')").is_ok());
        assert!(config.validate_code("df.eval('salary > 5000')").is_ok());
        assert!(config.validate_code("np.eval('x')").is_ok());
    }

    #[test]
    fn test_validate_code_re_compile_allowed() {
        let config = SandboxConfig::default();
        // re.compile() is a method call, not bare compile()
        assert!(config.validate_code("pattern = re.compile(r'\\d+')").is_ok());
        assert!(config.validate_code("regex.compile('test')").is_ok());
    }

    #[test]
    fn test_validate_code_bare_eval_still_blocked() {
        let config = SandboxConfig::default();
        // Bare eval() calls should still be blocked
        assert!(config.validate_code("eval('1+2')").is_err());
        assert!(config.validate_code("x = eval('code')").is_err());
        assert!(config.validate_code("result = (eval('x'))").is_err());
        assert!(config.validate_code("if eval('cond'):").is_err());
        // At start of line
        assert!(config.validate_code("eval('dangerous')").is_err());
    }

    #[test]
    fn test_validate_code_bare_compile_still_blocked() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("compile('code', '<string>', 'exec')").is_err());
        assert!(config.validate_code("x = compile('c', '', 'eval')").is_err());
    }

    #[test]
    fn test_validate_code_bare_exec_still_blocked() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("exec('import os')").is_err());
        assert!(config.validate_code("x; exec('code')").is_err());
    }

    // -- Relaxed modules (local desktop use) -----------------------------------

    #[test]
    fn test_validate_code_shutil_allowed() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("import shutil\nshutil.copy('a', 'b')").is_ok());
    }

    #[test]
    fn test_validate_code_requests_allowed() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("import requests\nr = requests.get('https://api.example.com')").is_ok());
    }

    #[test]
    fn test_validate_code_socket_allowed() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("import socket").is_ok());
    }

    #[test]
    fn test_validate_code_http_urllib_allowed() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("import http\nimport urllib").is_ok());
        assert!(config.validate_code("from urllib.request import urlopen").is_ok());
        assert!(config.validate_code("from http.client import HTTPConnection").is_ok());
    }

    // -- contains_standalone unit tests ----------------------------------------

    #[test]
    fn test_contains_standalone_basic() {
        assert!(contains_standalone("eval('x')", "eval("));
        assert!(contains_standalone("x = eval('y')", "eval("));
        assert!(contains_standalone("(eval('y'))", "eval("));
        assert!(!contains_standalone("pd.eval('x')", "eval("));
        assert!(!contains_standalone("df.eval('x')", "eval("));
        assert!(!contains_standalone("my_eval('x')", "eval("));
    }

    #[test]
    fn test_contains_standalone_compile() {
        assert!(contains_standalone("compile('c', '', 'exec')", "compile("));
        assert!(!contains_standalone("re.compile('pattern')", "compile("));
        assert!(!contains_standalone("jinja.compile('t')", "compile("));
    }
}
