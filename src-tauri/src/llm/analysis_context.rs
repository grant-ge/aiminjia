//! P2: Structured analysis context — persistent file profiles and step findings.
//!
//! Maintains a structured summary of uploaded files (column info, stats, row counts)
//! and step findings that is injected into the system prompt. This replaces the need
//! for the LLM to re-discover file structure via tool output on every iteration.
//!
//! **Persistence**: Serialized to `analysis/{conversation_id}/_analysis_ctx.json`
//! so it survives crashes and step transitions.

use log::{info, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Top-level analysis context, maintained per conversation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalysisContext {
    /// Per-file structural profiles.
    pub files: Vec<FileProfile>,
    /// Accumulated key findings from the current step.
    pub step_findings: Vec<Finding>,
    /// Free-form data insights accumulated across iterations.
    pub data_insights: Vec<String>,
    /// Column mapping result (from step 2 normalization), if available.
    pub column_mapping: Option<Value>,
    /// Current analysis step number.
    pub current_step: u32,
}

/// Structural profile of a single uploaded file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileProfile {
    pub file_id: String,
    pub original_name: String,
    pub row_count: usize,
    pub column_count: usize,
    pub columns: Vec<ColumnInfo>,
    pub numeric_stats: Vec<NumericStat>,
    /// The Python variable name hint (e.g., `_df` or `_dfs['file_id']`).
    pub variable_hint: String,
}

/// Column metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    pub dtype: String,
    /// Null percentage (0.0–100.0).
    pub null_pct: f64,
}

/// Numeric column statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NumericStat {
    pub column: String,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub median: f64,
}

/// A structured finding from analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    /// Category: data_quality, exclusion, diagnosis, normalization, etc.
    pub category: String,
    /// One-sentence summary.
    pub summary: String,
}

impl AnalysisContext {
    /// Persistence file path for a given conversation.
    fn persist_path(workspace: &Path, conversation_id: &str) -> PathBuf {
        workspace
            .join("analysis")
            .join(conversation_id)
            .join("_analysis_ctx.json")
    }

    /// Load from disk, or return a fresh default if not found.
    pub fn load_or_default(workspace: &Path, conversation_id: &str) -> Self {
        let path = Self::persist_path(workspace, conversation_id);
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(json) => match serde_json::from_str::<AnalysisContext>(&json) {
                    Ok(ctx) => {
                        info!(
                            "[AnalysisContext] Loaded from {:?} ({} files, step {})",
                            path,
                            ctx.files.len(),
                            ctx.current_step
                        );
                        return ctx;
                    }
                    Err(e) => warn!("[AnalysisContext] Failed to parse {:?}: {}", path, e),
                },
                Err(e) => warn!("[AnalysisContext] Failed to read {:?}: {}", path, e),
            }
        }
        Self::default()
    }

    /// Persist to disk (crash-safe: write tmp + rename).
    pub fn save(&self, workspace: &Path, conversation_id: &str) {
        let path = Self::persist_path(workspace, conversation_id);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = path.with_extension("json.tmp");
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&tmp, &json) {
                    warn!("[AnalysisContext] Failed to write tmp {:?}: {}", tmp, e);
                    return;
                }
                if let Err(e) = std::fs::rename(&tmp, &path) {
                    warn!("[AnalysisContext] Failed to rename {:?} → {:?}: {}", tmp, path, e);
                }
            }
            Err(e) => warn!("[AnalysisContext] Failed to serialize: {}", e),
        }
    }

    /// Update from a `load_file` tool result.
    ///
    /// Parses the structured output from `handle_load_file` to extract column names,
    /// types, row count, and basic statistics.
    pub fn update_from_load_file(
        &mut self,
        file_id: &str,
        original_name: &str,
        variable_hint: &str,
        tool_output: &str,
    ) {
        // Remove existing profile for this file (in case of re-load)
        self.files.retain(|f| f.file_id != file_id);

        let mut profile = FileProfile {
            file_id: file_id.to_string(),
            original_name: original_name.to_string(),
            row_count: 0,
            column_count: 0,
            columns: Vec::new(),
            numeric_stats: Vec::new(),
            variable_hint: variable_hint.to_string(),
        };

        // Parse row count: "行数: N" or "Rows: N" or "Shape: (N, M)"
        for line in tool_output.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("行数:").or_else(|| trimmed.strip_prefix("Rows:")) {
                if let Ok(n) = rest.trim().replace(',', "").parse::<usize>() {
                    profile.row_count = n;
                }
            }
            if let Some(rest) = trimmed.strip_prefix("列数:").or_else(|| trimmed.strip_prefix("Columns:")) {
                if let Ok(n) = rest.trim().replace(',', "").parse::<usize>() {
                    profile.column_count = n;
                }
            }
            if trimmed.starts_with("Shape:") || trimmed.starts_with("shape:") {
                // Shape: (1000, 15)
                if let Some(inner) = trimmed.split('(').nth(1).and_then(|s| s.split(')').next()) {
                    let parts: Vec<&str> = inner.split(',').collect();
                    if parts.len() == 2 {
                        profile.row_count = parts[0].trim().replace(',', "").parse().unwrap_or(0);
                        profile.column_count = parts[1].trim().replace(',', "").parse().unwrap_or(0);
                    }
                }
            }
        }

        // Parse column info from lines like "  column_name  dtype  null%"
        // This is a best-effort extraction — the exact format depends on the tool output
        self.extract_columns_from_output(tool_output, &mut profile);

        info!(
            "[AnalysisContext] Added file profile: {} ({} rows, {} cols, var={})",
            original_name, profile.row_count, profile.column_count, variable_hint
        );

        self.files.push(profile);
    }

    /// Extract column info from dtypes-like output.
    fn extract_columns_from_output(&self, output: &str, profile: &mut FileProfile) {
        // Look for "列名" section or dtypes section
        let mut in_columns = false;
        for line in output.lines() {
            let trimmed = line.trim();

            // Detect column section start
            if trimmed.contains("列名") || trimmed.contains("Column") || trimmed.contains("dtypes") {
                in_columns = true;
                continue;
            }

            // End of section: empty line or a different section header
            if in_columns && (trimmed.is_empty() || trimmed.starts_with("---") || trimmed.starts_with("===")) {
                if !profile.columns.is_empty() {
                    in_columns = false;
                    continue;
                }
            }

            if in_columns {
                // Try to parse "col_name   dtype   null%" or "col_name: dtype"
                let parts: Vec<&str> = trimmed.splitn(3, |c: char| c.is_whitespace()).collect();
                if parts.len() >= 2 {
                    let name = parts[0].trim_end_matches(':').to_string();
                    let dtype = parts[1].to_string();
                    let null_pct = if parts.len() >= 3 {
                        parts[2].trim_end_matches('%').parse::<f64>().unwrap_or(0.0)
                    } else {
                        0.0
                    };

                    // Skip lines that look like headers
                    if name == "Column" || name == "列名" || name == "---" || name == "===" {
                        continue;
                    }

                    profile.columns.push(ColumnInfo { name, dtype, null_pct });
                }
            }
        }
    }

    /// Update from a `execute_python` tool result.
    ///
    /// Looks for structured markers or patterns:
    /// - `__ANALYSIS_FINDING__:{"category":"...","summary":"..."}`
    /// - `__DATA_INSIGHT__:...`
    /// - `__COLUMN_MAPPING__:{...}`
    pub fn update_from_python_output(&mut self, tool_output: &str) {
        for line in tool_output.lines() {
            let trimmed = line.trim();

            if let Some(json_str) = trimmed.strip_prefix("__ANALYSIS_FINDING__:") {
                if let Ok(finding) = serde_json::from_str::<Finding>(json_str) {
                    self.step_findings.push(finding);
                }
            }

            if let Some(insight) = trimmed.strip_prefix("__DATA_INSIGHT__:") {
                let text = insight.trim().to_string();
                if !text.is_empty() && !self.data_insights.contains(&text) {
                    self.data_insights.push(text);
                }
            }

            if let Some(json_str) = trimmed.strip_prefix("__COLUMN_MAPPING__:") {
                if let Ok(mapping) = serde_json::from_str::<Value>(json_str) {
                    self.column_mapping = Some(mapping);
                }
            }

            // Heuristic: detect row count info from df.shape or len() output
            // e.g., "(1000, 15)" or "1000 rows × 15 columns"
            if (trimmed.contains("rows") && trimmed.contains("columns"))
                || (trimmed.starts_with('(') && trimmed.contains(','))
            {
                self.try_update_row_count(trimmed);
            }
        }
    }

    /// Try to update file row counts from shape-like output.
    fn try_update_row_count(&mut self, line: &str) {
        // Pattern: "(N, M)" or "N rows × M columns"
        if let Some(inner) = line.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
            let parts: Vec<&str> = inner.split(',').collect();
            if parts.len() == 2 {
                if let Ok(rows) = parts[0].trim().parse::<usize>() {
                    // Update the first file that has mismatched row count
                    if let Some(f) = self.files.first_mut() {
                        if f.row_count == 0 {
                            f.row_count = rows;
                        }
                    }
                }
            }
        }
    }

    /// Format the context for injection into the system prompt.
    ///
    /// Returns a section like:
    /// ```text
    /// [分析上下文]
    /// ## 文件画像
    /// ### file.xlsx (1000行, 15列) → _df
    /// 列: name(object), salary(float64, 0.0% null), ...
    /// 数值统计: salary(min=20000, max=95000, mean=50000)
    ///
    /// ## 当前发现
    /// - [data_quality] 薪资列有 5% 的空值
    /// ```
    pub fn format_for_prompt(&self) -> String {
        if self.files.is_empty() && self.step_findings.is_empty() && self.data_insights.is_empty() {
            return String::new();
        }

        let mut out = String::from("\n\n[分析上下文]\n");

        if !self.files.is_empty() {
            out.push_str("## 文件画像\n");
            for f in &self.files {
                out.push_str(&format!(
                    "### {} ({}行, {}列) → {}\n",
                    f.original_name, f.row_count, f.column_count, f.variable_hint
                ));

                if !f.columns.is_empty() {
                    out.push_str("列: ");
                    let col_strs: Vec<String> = f.columns.iter().take(30).map(|c| {
                        if c.null_pct > 0.0 {
                            format!("{}({}, {:.1}% null)", c.name, c.dtype, c.null_pct)
                        } else {
                            format!("{}({})", c.name, c.dtype)
                        }
                    }).collect();
                    out.push_str(&col_strs.join(", "));
                    if f.columns.len() > 30 {
                        out.push_str(&format!(" ...+{} more", f.columns.len() - 30));
                    }
                    out.push('\n');
                }

                if !f.numeric_stats.is_empty() {
                    out.push_str("数值统计: ");
                    let stat_strs: Vec<String> = f.numeric_stats.iter().take(10).map(|s| {
                        format!(
                            "{}(min={:.0}, max={:.0}, mean={:.0}, median={:.0})",
                            s.column, s.min, s.max, s.mean, s.median
                        )
                    }).collect();
                    out.push_str(&stat_strs.join(", "));
                    out.push('\n');
                }
                out.push('\n');
            }
        }

        if !self.step_findings.is_empty() {
            out.push_str("## 关键发现\n");
            for f in &self.step_findings {
                out.push_str(&format!("- [{}] {}\n", f.category, f.summary));
            }
            out.push('\n');
        }

        if !self.data_insights.is_empty() {
            out.push_str("## 数据洞察\n");
            for insight in &self.data_insights {
                out.push_str(&format!("- {}\n", insight));
            }
            out.push('\n');
        }

        if let Some(ref mapping) = self.column_mapping {
            out.push_str("## 列映射\n");
            out.push_str(&serde_json::to_string_pretty(mapping).unwrap_or_default());
            out.push('\n');
        }

        out
    }

    /// Clear step-specific findings (called on step transitions).
    pub fn advance_step(&mut self, new_step: u32) {
        let old_step = self.current_step;
        self.current_step = new_step;
        // Move step_findings to data_insights (persist as one-liners)
        for finding in self.step_findings.drain(..) {
            let insight = format!("[step{}:{}] {}", old_step, finding.category, finding.summary);
            if !self.data_insights.contains(&insight) {
                self.data_insights.push(insight);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path();
        let conv_id = "test-conv-123";

        let mut ctx = AnalysisContext::default();
        ctx.current_step = 2;
        ctx.files.push(FileProfile {
            file_id: "f1".to_string(),
            original_name: "data.xlsx".to_string(),
            row_count: 1000,
            column_count: 15,
            columns: vec![ColumnInfo {
                name: "salary".to_string(),
                dtype: "float64".to_string(),
                null_pct: 2.5,
            }],
            numeric_stats: vec![NumericStat {
                column: "salary".to_string(),
                min: 20000.0,
                max: 95000.0,
                mean: 50000.0,
                median: 48000.0,
            }],
            variable_hint: "_df".to_string(),
        });
        ctx.step_findings.push(Finding {
            category: "data_quality".to_string(),
            summary: "薪资列有 2.5% 空值".to_string(),
        });

        ctx.save(workspace, conv_id);

        let loaded = AnalysisContext::load_or_default(workspace, conv_id);
        assert_eq!(loaded.current_step, 2);
        assert_eq!(loaded.files.len(), 1);
        assert_eq!(loaded.files[0].original_name, "data.xlsx");
        assert_eq!(loaded.files[0].row_count, 1000);
        assert_eq!(loaded.step_findings.len(), 1);
    }

    #[test]
    fn load_default_when_no_file() {
        let tmp = TempDir::new().unwrap();
        let ctx = AnalysisContext::load_or_default(tmp.path(), "nonexistent");
        assert_eq!(ctx.current_step, 0);
        assert!(ctx.files.is_empty());
    }

    #[test]
    fn update_from_load_file_parses_shape() {
        let mut ctx = AnalysisContext::default();
        let output = "Shape: (1000, 15)\n行数: 1000\n列数: 15\n";
        ctx.update_from_load_file("f1", "test.xlsx", "_df", output);

        assert_eq!(ctx.files.len(), 1);
        assert_eq!(ctx.files[0].row_count, 1000);
        assert_eq!(ctx.files[0].column_count, 15);
    }

    #[test]
    fn update_from_python_output_extracts_findings() {
        let mut ctx = AnalysisContext::default();
        let output = r#"Processing...
__ANALYSIS_FINDING__:{"category":"data_quality","summary":"5% null in salary"}
__DATA_INSIGHT__:Salary range is 20k-95k
Done"#;
        ctx.update_from_python_output(output);

        assert_eq!(ctx.step_findings.len(), 1);
        assert_eq!(ctx.step_findings[0].category, "data_quality");
        assert_eq!(ctx.data_insights.len(), 1);
        assert!(ctx.data_insights[0].contains("Salary range"));
    }

    #[test]
    fn format_for_prompt_output() {
        let mut ctx = AnalysisContext::default();
        ctx.files.push(FileProfile {
            file_id: "f1".to_string(),
            original_name: "data.xlsx".to_string(),
            row_count: 1000,
            column_count: 3,
            columns: vec![
                ColumnInfo { name: "name".to_string(), dtype: "object".to_string(), null_pct: 0.0 },
                ColumnInfo { name: "salary".to_string(), dtype: "float64".to_string(), null_pct: 2.5 },
            ],
            numeric_stats: vec![],
            variable_hint: "_df".to_string(),
        });
        ctx.step_findings.push(Finding {
            category: "data_quality".to_string(),
            summary: "Some nulls found".to_string(),
        });

        let prompt = ctx.format_for_prompt();
        assert!(prompt.contains("[分析上下文]"));
        assert!(prompt.contains("data.xlsx"));
        assert!(prompt.contains("1000行"));
        assert!(prompt.contains("salary(float64, 2.5% null)"));
        assert!(prompt.contains("[data_quality]"));
    }

    #[test]
    fn empty_context_returns_empty_string() {
        let ctx = AnalysisContext::default();
        assert_eq!(ctx.format_for_prompt(), "");
    }

    #[test]
    fn advance_step_moves_findings_to_insights() {
        let mut ctx = AnalysisContext::default();
        ctx.current_step = 1;
        ctx.step_findings.push(Finding {
            category: "diagnosis".to_string(),
            summary: "pay gap found".to_string(),
        });

        ctx.advance_step(2);
        assert!(ctx.step_findings.is_empty());
        assert_eq!(ctx.data_insights.len(), 1);
        assert!(ctx.data_insights[0].contains("pay gap found"));
        assert_eq!(ctx.current_step, 2);
    }

    #[test]
    fn multi_file_profiles() {
        let mut ctx = AnalysisContext::default();
        ctx.update_from_load_file("f1", "salary.xlsx", "_dfs['f1']", "Shape: (500, 10)\n");
        ctx.update_from_load_file("f2", "bonus.xlsx", "_dfs['f2']", "Shape: (300, 8)\n");

        assert_eq!(ctx.files.len(), 2);

        let prompt = ctx.format_for_prompt();
        assert!(prompt.contains("salary.xlsx"));
        assert!(prompt.contains("bonus.xlsx"));
        assert!(prompt.contains("_dfs['f1']"));
        assert!(prompt.contains("_dfs['f2']"));
    }

    #[test]
    fn reload_same_file_replaces_profile() {
        let mut ctx = AnalysisContext::default();
        ctx.update_from_load_file("f1", "data.xlsx", "_df", "Shape: (500, 10)\n");
        assert_eq!(ctx.files[0].row_count, 500);

        // Re-load same file with updated data
        ctx.update_from_load_file("f1", "data.xlsx", "_df", "Shape: (1000, 12)\n");
        assert_eq!(ctx.files.len(), 1);
        assert_eq!(ctx.files[0].row_count, 1000);
    }

    #[test]
    fn column_mapping_update() {
        let mut ctx = AnalysisContext::default();
        let output = r#"__COLUMN_MAPPING__:{"salary":"base_pay","name":"employee_name"}"#;
        ctx.update_from_python_output(output);

        assert!(ctx.column_mapping.is_some());
        let mapping = ctx.column_mapping.as_ref().unwrap();
        assert_eq!(mapping["salary"], "base_pay");
    }
}
