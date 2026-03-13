# Step 3: Root cause diagnosis - correlation analysis for low-scoring dimensions
# Executed automatically by Rust before LLM starts.
# Depends on: _df, _ANALYSIS_DIR, _export_detail

import json as _json_mod
import pandas as _pd_mod

try:
    step1_path = os.path.join(_ANALYSIS_DIR, 'step1_precompute.json')
    with open(step1_path, 'r') as f:
        step1 = _json_mod.load(f)
except (FileNotFoundError, _json_mod.JSONDecodeError):
    step1 = {}

dimension_cols = step1.get('dimension_columns', [])
dimension_scores = step1.get('dimension_scores', [])

# --- Identify bottom dimensions ---
bottom_dims = [d['dimension'] for d in dimension_scores[-3:]] if len(dimension_scores) >= 3 else [d['dimension'] for d in dimension_scores]

# --- Correlation matrix ---
correlation_data = []
if len(dimension_cols) >= 2:
    numeric_df = _df[dimension_cols].apply(_pd_mod.to_numeric, errors='coerce')
    corr_matrix = numeric_df.corr()

    for dim in bottom_dims:
        if dim in corr_matrix.columns:
            correlations = corr_matrix[dim].drop(dim, errors='ignore')
            top_correlated = correlations.abs().sort_values(ascending=False).head(5)
            for other_dim, corr_val in top_correlated.items():
                correlation_data.append({
                    'low_dimension': dim,
                    'correlated_with': other_dim,
                    'correlation': round(float(corr_val), 3),
                    'strength': '强' if abs(corr_val) > 0.7 else ('中' if abs(corr_val) > 0.4 else '弱'),
                })

# --- Engagement driver analysis ---
# Calculate which dimensions most strongly predict overall engagement
driver_analysis = []
if dimension_cols:
    numeric_df = _df[dimension_cols].apply(_pd_mod.to_numeric, errors='coerce')
    overall = numeric_df.mean(axis=1)
    for dim in dimension_cols:
        dim_scores = numeric_df[dim].dropna()
        valid_idx = dim_scores.index.intersection(overall.dropna().index)
        if len(valid_idx) > 10:
            corr = dim_scores[valid_idx].corr(overall[valid_idx])
            dim_info = next((d for d in dimension_scores if d['dimension'] == dim), {})
            driver_analysis.append({
                'dimension': dim,
                'correlation_with_overall': round(float(corr), 3),
                'current_score': dim_info.get('mean', 0),
                'impact': '高' if abs(corr) > 0.7 else ('中' if abs(corr) > 0.5 else '低'),
            })
    driver_analysis.sort(key=lambda x: abs(x['correlation_with_overall']), reverse=True)

_precompute = {
    'bottom_dimensions': bottom_dims,
    'correlations': correlation_data,
    'driver_analysis': driver_analysis,
}

with open(os.path.join(_ANALYSIS_DIR, 'step3_precompute.json'), 'w') as f:
    _json_mod.dump(_precompute, f, ensure_ascii=False, default=str)
print(_json_mod.dumps(_precompute, ensure_ascii=False, default=str, indent=2))

if driver_analysis:
    _export_detail(_pd_mod.DataFrame(driver_analysis), 'step3_root_cause', '根因诊断分析')
