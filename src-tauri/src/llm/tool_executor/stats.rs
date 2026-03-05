//! hypothesis_test and detect_anomalies handlers + Python code generation.

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::plugin::context::PluginContext;
use crate::python::runner::PythonRunner;

use super::{optional_str, require_str};
use super::optional_f64;
use super::util::{py_escape, indent_python};

/// 6. hypothesis_test — run a statistical hypothesis test via Python.
pub(crate) async fn handle_hypothesis_test(ctx: &PluginContext, args: &Value) -> Result<String> {
    let test_type = require_str(args, "test_type")?;
    let groups = args
        .get("groups")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("Missing required array argument: groups"))?;
    let data_source = optional_str(args, "data_source");
    let significance_level = optional_f64(args, "significance_level", 0.05);

    let group_names: Vec<&str> = groups.iter().filter_map(|v| v.as_str()).collect();

    let python_code =
        build_hypothesis_test_python(test_type, &group_names, data_source, significance_level)?;

    let runner = PythonRunner::new(ctx.workspace_path.clone(), ctx.app_handle.as_ref());
    let result = runner.execute(&python_code).await?;

    if result.exit_code != 0 {
        return Err(anyhow!(
            "Hypothesis test failed:\n{}",
            if result.stderr.is_empty() {
                &result.stdout
            } else {
                &result.stderr
            }
        ));
    }

    Ok(result.stdout)
}

/// 7. detect_anomalies — find outliers via Z-score, IQR, or Grubbs.
pub(crate) async fn handle_detect_anomalies(ctx: &PluginContext, args: &Value) -> Result<String> {
    let column = require_str(args, "column")?;
    let method = optional_str(args, "method").unwrap_or("zscore");
    let threshold = optional_f64(args, "threshold", 3.0);
    let group_by = optional_str(args, "group_by");

    let python_code = build_anomaly_detection_python(column, method, threshold, group_by)?;

    let runner = PythonRunner::new(ctx.workspace_path.clone(), ctx.app_handle.as_ref());
    let result = runner.execute(&python_code).await?;

    if result.exit_code != 0 {
        return Err(anyhow!(
            "Anomaly detection failed:\n{}",
            if result.stderr.is_empty() {
                &result.stdout
            } else {
                &result.stderr
            }
        ));
    }

    Ok(result.stdout)
}

fn build_hypothesis_test_python(
    test_type: &str,
    groups: &[&str],
    data_source: Option<&str>,
    significance_level: f64,
) -> Result<String> {
    let groups_json =
        serde_json::to_string(groups).unwrap_or_else(|_| "[]".to_string());

    let load_data = if let Some(source) = data_source {
        let escaped_source = py_escape(source);
        format!(
            r#"
import pandas as pd
import os

# Try to load from source
source = r'{source}'
if os.path.isfile(source):
    df = _smart_read_data(source)
else:
    # Assume source is a file ID and look in uploads/
    import glob
    files = glob.glob(f'uploads/*')
    if files:
        f = files[0]
        df = _smart_read_data(f)
    else:
        raise FileNotFoundError(f"No data file found for source: {{source}}")
"#,
            source = escaped_source,
        )
    } else {
        r#"
import pandas as pd
import glob

# Auto-detect data file in uploads
files = glob.glob('uploads/*')
if not files:
    raise FileNotFoundError("No data files found in uploads/")
f = files[0]
df = pd.read_csv(f) if f.endswith('.csv') else pd.read_excel(f)
"#
        .to_string()
    };

    let test_code = match test_type {
        "t_test" => format!(
            r#"
from scipy import stats
group_cols = {groups}
if len(group_cols) < 2:
    raise ValueError("t-test requires at least 2 groups")
g1 = df[group_cols[0]].dropna()
g2 = df[group_cols[1]].dropna()
stat, p_value = stats.ttest_ind(g1, g2)
print(f"T-test: t-statistic={{stat:.4f}}, p-value={{p_value:.6f}}")
print(f"Significant at alpha={alpha}: {{p_value < {alpha}}}")
print(f"Group 1 ({{group_cols[0]}}): mean={{g1.mean():.4f}}, std={{g1.std():.4f}}, n={{len(g1)}}")
print(f"Group 2 ({{group_cols[1]}}): mean={{g2.mean():.4f}}, std={{g2.std():.4f}}, n={{len(g2)}}")
"#,
            groups = groups_json,
            alpha = significance_level,
        ),
        "anova" => format!(
            r#"
from scipy import stats
group_cols = {groups}
group_data = [df[col].dropna().values for col in group_cols]
stat, p_value = stats.f_oneway(*group_data)
print(f"ANOVA: F-statistic={{stat:.4f}}, p-value={{p_value:.6f}}")
print(f"Significant at alpha={alpha}: {{p_value < {alpha}}}")
for col in group_cols:
    vals = df[col].dropna()
    print(f"  {{col}}: mean={{vals.mean():.4f}}, std={{vals.std():.4f}}, n={{len(vals)}}")
"#,
            groups = groups_json,
            alpha = significance_level,
        ),
        "chi_square" => format!(
            r#"
from scipy import stats
import numpy as np
group_cols = {groups}
if len(group_cols) < 2:
    raise ValueError("Chi-square test requires at least 2 columns")
contingency = pd.crosstab(df[group_cols[0]], df[group_cols[1]])
stat, p_value, dof, expected = stats.chi2_contingency(contingency)
print(f"Chi-square test: statistic={{stat:.4f}}, p-value={{p_value:.6f}}, dof={{dof}}")
print(f"Significant at alpha={alpha}: {{p_value < {alpha}}}")
print(f"Contingency table:\n{{contingency}}")
"#,
            groups = groups_json,
            alpha = significance_level,
        ),
        "regression" => format!(
            r#"
from scipy import stats
group_cols = {groups}
if len(group_cols) < 2:
    raise ValueError("Regression requires at least 2 columns (x, y)")
x = df[group_cols[0]].dropna()
y = df[group_cols[1]].dropna()
# Align indices
common = x.index.intersection(y.index)
x, y = x[common], y[common]
slope, intercept, r_value, p_value, std_err = stats.linregress(x, y)
print(f"Linear Regression:")
print(f"  slope={{slope:.4f}}, intercept={{intercept:.4f}}")
print(f"  R-squared={{r_value**2:.4f}}")
print(f"  p-value={{p_value:.6f}}, std_err={{std_err:.4f}}")
print(f"  Significant at alpha={alpha}: {{p_value < {alpha}}}")
"#,
            groups = groups_json,
            alpha = significance_level,
        ),
        "mann_whitney" => format!(
            r#"
from scipy import stats
group_cols = {groups}
if len(group_cols) < 2:
    raise ValueError("Mann-Whitney test requires at least 2 groups")
g1 = df[group_cols[0]].dropna()
g2 = df[group_cols[1]].dropna()
stat, p_value = stats.mannwhitneyu(g1, g2, alternative='two-sided')
print(f"Mann-Whitney U test: U-statistic={{stat:.4f}}, p-value={{p_value:.6f}}")
print(f"Significant at alpha={alpha}: {{p_value < {alpha}}}")
print(f"Group 1 ({{group_cols[0]}}): median={{g1.median():.4f}}, n={{len(g1)}}")
print(f"Group 2 ({{group_cols[1]}}): median={{g2.median():.4f}}, n={{len(g2)}}")
"#,
            groups = groups_json,
            alpha = significance_level,
        ),
        other => {
            return Err(anyhow!(
                "Unsupported test type: {}. Supported: t_test, anova, chi_square, regression, mann_whitney",
                other
            ));
        }
    };

    let load_data = indent_python(&load_data, 4);
    let test_code = indent_python(&test_code, 4);

    Ok(format!(
        r#"
import json
import sys
import warnings
warnings.filterwarnings('ignore')

try:
{load_data}
{test_code}
except Exception as e:
    print(f"Error: {{e}}", file=sys.stderr)
    sys.exit(1)
"#,
        load_data = load_data,
        test_code = test_code,
    ))
}

/// Generate Python code for anomaly detection.
fn build_anomaly_detection_python(
    column: &str,
    method: &str,
    threshold: f64,
    group_by: Option<&str>,
) -> Result<String> {
    let escaped_column = py_escape(column);
    let detection_code = match method {
        "zscore" => format!(
            r#"
from scipy import stats
import numpy as np

def detect_zscore(series, threshold):
    z_scores = np.abs(stats.zscore(series.dropna()))
    mask = z_scores > threshold
    anomalies = series.dropna()[mask]
    return anomalies, z_scores

col = '{column}'
threshold = {threshold}

if group_by:
    for group_name, group_df in df.groupby(group_by):
        data = group_df[col].dropna()
        if len(data) < 3:
            continue
        anomalies, z_scores = detect_zscore(data, threshold)
        print(f"Group '{{group_name}}': {{len(anomalies)}} anomalies out of {{len(data)}} values")
        if len(anomalies) > 0:
            print(f"  Anomalous values: {{anomalies.tolist()}}")
else:
    data = df[col].dropna()
    anomalies, z_scores = detect_zscore(data, threshold)
    print(f"Column '{{col}}': {{len(anomalies)}} anomalies out of {{len(data)}} values (z-score > {{threshold}})")
    if len(anomalies) > 0:
        print(f"  Anomalous values: {{anomalies.tolist()}}")
    print(f"  Mean: {{data.mean():.4f}}, Std: {{data.std():.4f}}")
    print(f"  Min: {{data.min():.4f}}, Max: {{data.max():.4f}}")
"#,
            column = escaped_column,
            threshold = threshold,
        ),
        "iqr" => format!(
            r#"
import numpy as np

def detect_iqr(series, multiplier):
    q1 = series.quantile(0.25)
    q3 = series.quantile(0.75)
    iqr = q3 - q1
    lower = q1 - multiplier * iqr
    upper = q3 + multiplier * iqr
    anomalies = series[(series < lower) | (series > upper)]
    return anomalies, lower, upper

col = '{column}'
multiplier = {threshold}

if group_by:
    for group_name, group_df in df.groupby(group_by):
        data = group_df[col].dropna()
        if len(data) < 4:
            continue
        anomalies, lower, upper = detect_iqr(data, multiplier)
        print(f"Group '{{group_name}}': {{len(anomalies)}} anomalies, bounds=[{{lower:.4f}}, {{upper:.4f}}]")
        if len(anomalies) > 0:
            print(f"  Anomalous values: {{anomalies.tolist()}}")
else:
    data = df[col].dropna()
    anomalies, lower, upper = detect_iqr(data, multiplier)
    print(f"Column '{{col}}': {{len(anomalies)}} anomalies (IQR multiplier={{multiplier}})")
    print(f"  Bounds: [{{lower:.4f}}, {{upper:.4f}}]")
    if len(anomalies) > 0:
        print(f"  Anomalous values: {{anomalies.tolist()}}")
    print(f"  Q1: {{data.quantile(0.25):.4f}}, Q3: {{data.quantile(0.75):.4f}}")
"#,
            column = escaped_column,
            threshold = threshold,
        ),
        "grubbs" => format!(
            r#"
from scipy import stats
import numpy as np

def grubbs_test(data, alpha=0.05):
    n = len(data)
    mean = np.mean(data)
    std = np.std(data, ddof=1)
    if std == 0:
        return [], []
    abs_dev = np.abs(data - mean)
    max_idx = np.argmax(abs_dev)
    G = abs_dev[max_idx] / std
    t_crit = stats.t.ppf(1 - alpha / (2 * n), n - 2)
    G_crit = (n - 1) / np.sqrt(n) * np.sqrt(t_crit**2 / (n - 2 + t_crit**2))
    if G > G_crit:
        return [data.iloc[max_idx]], [max_idx]
    return [], []

col = '{column}'

if group_by:
    for group_name, group_df in df.groupby(group_by):
        data = group_df[col].dropna()
        if len(data) < 3:
            continue
        anomalies, indices = grubbs_test(data)
        print(f"Group '{{group_name}}': {{len(anomalies)}} anomalies detected by Grubbs test")
        if anomalies:
            print(f"  Anomalous values: {{anomalies}}")
else:
    data = df[col].dropna()
    anomalies, indices = grubbs_test(data)
    print(f"Column '{{col}}': {{len(anomalies)}} anomalies detected by Grubbs test")
    if anomalies:
        print(f"  Anomalous values: {{anomalies}}")
    print(f"  Mean: {{data.mean():.4f}}, Std: {{data.std():.4f}}, n={{len(data)}}")
"#,
            column = escaped_column,
        ),
        other => {
            return Err(anyhow!(
                "Unsupported anomaly detection method: {}. Supported: zscore, iqr, grubbs",
                other
            ));
        }
    };

    let group_by_code = if let Some(gb) = group_by {
        format!("group_by = '{}'", py_escape(gb))
    } else {
        "group_by = None".to_string()
    };

    let group_by_code = indent_python(&group_by_code, 4);
    let detection_code = indent_python(&detection_code, 4);

    Ok(format!(
        r#"
import pandas as pd
import json
import sys
import glob
import warnings
warnings.filterwarnings('ignore')

try:
    # Auto-detect data file
    files = glob.glob('uploads/*')
    if not files:
        raise FileNotFoundError("No data files found in uploads/")
    f = files[0]
    df = pd.read_csv(f) if f.endswith('.csv') else pd.read_excel(f)

{group_by_code}
{detection_code}
except Exception as e:
    print(f"Error: {{e}}", file=sys.stderr)
    sys.exit(1)
"#,
        group_by_code = group_by_code,
        detection_code = detection_code,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_hypothesis_test_python_ttest() {
        let groups = vec!["col_a", "col_b"];
        let code = build_hypothesis_test_python("t_test", &groups, None, 0.05).unwrap();
        assert!(code.contains("ttest_ind"));
        assert!(code.contains("col_a"));
        assert!(code.contains("col_b"));
        assert!(code.contains("alpha=0.05"));
    }

    #[test]
    fn test_build_hypothesis_test_python_unsupported() {
        let groups = vec!["a", "b"];
        let result = build_hypothesis_test_python("nonexistent_test", &groups, None, 0.05);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Unsupported test type"));
    }

    #[test]
    fn test_build_anomaly_detection_zscore() {
        let code = build_anomaly_detection_python("salary", "zscore", 3.0, None).unwrap();
        assert!(code.contains("detect_zscore"));
        assert!(code.contains("salary"));
        assert!(code.contains("threshold = 3"));
    }

    #[test]
    fn test_build_anomaly_detection_iqr() {
        let code = build_anomaly_detection_python("salary", "iqr", 1.5, Some("department")).unwrap();
        assert!(code.contains("detect_iqr"));
        assert!(code.contains("salary"));
        assert!(code.contains("multiplier = 1.5"));
        assert!(code.contains("group_by = 'department'"));
    }

    #[test]
    fn test_build_anomaly_detection_unsupported() {
        let result = build_anomaly_detection_python("col", "magic", 1.0, None);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Unsupported anomaly detection method"));
    }
}
