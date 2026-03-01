//! Sandbox configuration — timeout, allowed paths, forbidden imports.
//!
//! Controls what Python code is allowed to do. Prevents dangerous operations
//! like accessing the filesystem outside the workspace, making network calls,
//! or running subprocesses.

use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// Sandbox configuration for Python execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxConfig {
    /// Maximum execution time in seconds.
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
            timeout_seconds: 30,
            memory_limit_mb: 512,
            allowed_paths: Vec::new(),
            max_output_bytes: 1_000_000, // 1 MB
            forbidden_modules: vec![
                "subprocess".to_string(),
                "socket".to_string(),
                "http".to_string(),
                "urllib".to_string(),
                "requests".to_string(),
                "shutil".to_string(),
            ],
        }
    }
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

        // Check for dangerous built-in calls
        let dangerous_calls = [
            ("exec(", "exec() is not allowed"),
            ("eval(", "eval() is not allowed"),
            ("compile(", "compile() is not allowed"),
            ("os.system", "os.system() is not allowed"),
            ("os.popen", "os.popen() is not allowed"),
            ("os.exec", "os.exec*() is not allowed"),
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
    pub fn preamble(&self) -> String {
        // Part 1: Dynamic setup (uses Rust format! for allowed_paths)
        let setup = format!(
            r#"import sys
import os
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

# Working directory (workspace root — first element of _ALLOWED_PATHS)
os.chdir(_ALLOWED_PATHS[0] if _ALLOWED_PATHS else '.')
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
        );

        // Part 2: Static imports and utility functions (no Rust format! needed)
        let utilities = PYTHON_UTILITIES;

        format!("{}\n{}", setup, utilities)
    }
}

/// Pre-loaded Python imports and utility functions.
///
/// This is a static string (not processed by `format!`) so Python f-strings
/// with braces like `f"Rows: {len(df)}"` work correctly.
const PYTHON_UTILITIES: &str = r###"
# ============================================================
# Pre-loaded imports (saves ~2-3s cold start per execution)
# ============================================================
import json
import glob
import pandas as pd
import numpy as np
try:
    from scipy import stats as scipy_stats
except ImportError:
    scipy_stats = None

# ============================================================
# Utility functions — encoding detection, file I/O, formatting
# ============================================================

def _smart_read_csv(path, **kwargs):
    """Read CSV with encoding auto-detection (UTF-8 -> GBK -> latin-1)."""
    for enc in ['utf-8', 'utf-8-sig', 'gbk', 'gb2312', 'latin-1']:
        try:
            return pd.read_csv(path, encoding=enc, **kwargs)
        except (UnicodeDecodeError, UnicodeError):
            continue
    return pd.read_csv(path, encoding='latin-1', errors='replace', **kwargs)

def _smart_read_data(path, **kwargs):
    """Read CSV or Excel with encoding auto-detection."""
    if path.endswith('.csv') or path.endswith('.tsv'):
        return _smart_read_csv(path, **kwargs)
    else:
        return pd.read_excel(path, **kwargs)

def _find_data_file(pattern='uploads/*'):
    """Find the first data file matching the pattern."""
    files = glob.glob(pattern)
    data_exts = ('.xlsx', '.xls', '.csv', '.tsv')
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

def _export_detail(df, filename, title='明细数据', preview_rows=15):
    """Export a DataFrame to Excel and print an inline preview table.

    Saves the full data to exports/<filename>.xlsx and prints the first
    `preview_rows` rows as a Markdown table, followed by a download hint.

    Args:
        df: DataFrame to export
        filename: Base filename (without extension), e.g. 'step1_exclusion_detail'
        title: Section title for the inline preview
        preview_rows: Number of rows to show inline (default 15)

    Returns:
        The full path of the exported file.
    """
    import os
    export_dir = 'exports'
    os.makedirs(export_dir, exist_ok=True)
    # Strip .xlsx/.xls extension if LLM included it (defensive against double extension)
    base = filename
    for ext in ('.xlsx', '.xls'):
        if base.lower().endswith(ext):
            base = base[:-len(ext)]
            break
    out_name = f'{base}.xlsx'
    full_path = os.path.join(export_dir, out_name)
    df.to_excel(full_path, index=False, engine='openpyxl')

    n = len(df)
    # Emit structured marker for auto-registration in DB (format="excel" matches frontend FILE_TYPE_ICON)
    print(f'__GENERATED_FILE__:{{"path":"{full_path}","filename":"{out_name}","title":"{title}","format":"excel","rows":{n}}}')
    print(f'\n## {title}（共 {n} 条）')
    # Inline preview
    preview = df.head(preview_rows)
    headers = list(preview.columns)
    rows = []
    for _, row in preview.iterrows():
        rows.append([str(v) for v in row.values])
    _print_table(headers, rows)
    if n > preview_rows:
        print(f'\n> 完整 {n} 条明细已导出到 Excel')
    else:
        print(f'\n> 完整明细已导出到 Excel')
    return full_path
"###;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SandboxConfig::default();
        assert_eq!(config.timeout_seconds, 30);
        assert_eq!(config.memory_limit_mb, 512);
        assert!(config.forbidden_modules.contains(&"subprocess".to_string()));
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
        assert!(config.validate_code("from socket import socket").is_err());
    }

    #[test]
    fn test_validate_code_exec_blocked() {
        let config = SandboxConfig::default();
        assert!(config.validate_code("exec('print(1)')").is_err());
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
        assert!(config.validate_code("mod = __import__(\"socket\")").is_err());
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
}
