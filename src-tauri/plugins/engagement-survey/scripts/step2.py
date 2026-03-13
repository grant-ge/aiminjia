# Step 2: Group comparison analysis
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

detected = step1.get('detected_fields', {})
dimension_cols = step1.get('dimension_columns', [])

def _group_analysis(group_col, group_name):
    """Analyze dimension scores by group"""
    results = []
    if group_col not in _df.columns:
        return results
    groups = _df[group_col].dropna().unique()
    for grp in sorted(groups, key=str):
        grp_df = _df[_df[group_col] == grp]
        if len(grp_df) < 3:  # Skip groups with too few responses
            continue
        row = {'group_type': group_name, 'group': str(grp), 'count': len(grp_df)}
        dim_means = []
        for dim in dimension_cols:
            scores = _pd_mod.to_numeric(grp_df[dim], errors='coerce').dropna()
            if len(scores) > 0:
                mean = round(float(scores.mean()), 2)
                row[dim] = mean
                dim_means.append(mean)
        if dim_means:
            row['overall'] = round(sum(dim_means) / len(dim_means), 2)
        results.append(row)
    return results

# Analyze by department
dept_results = _group_analysis(detected.get('department', ''), '部门') if 'department' in detected else []

# Analyze by level
level_results = _group_analysis(detected.get('level', ''), '职级') if 'level' in detected else []

# Analyze by tenure
tenure_results = []
if 'tenure' in detected:
    tenure_col = detected['tenure']
    tenure_numeric = _pd_mod.to_numeric(_df[tenure_col], errors='coerce')
    if tenure_numeric.notna().sum() > 0:
        bins = [0, 1, 3, 5, 10, 100]
        labels = ['1年以下', '1-3年', '3-5年', '5-10年', '10年以上']
        _df['_tenure_group'] = _pd_mod.cut(tenure_numeric, bins=bins, labels=labels, right=False)
        tenure_results = _group_analysis('_tenure_group', '司龄段')

# Find significant gaps
all_results = dept_results + level_results + tenure_results
significant_gaps = []
if all_results:
    for r in all_results:
        overall = r.get('overall', 0)
        avg_overall = step1.get('overall_engagement_score', 0)
        if avg_overall and abs(overall - avg_overall) > 0.5:
            significant_gaps.append({
                'group_type': r['group_type'],
                'group': r['group'],
                'score': overall,
                'gap': round(overall - avg_overall, 2),
                'direction': '高于' if overall > avg_overall else '低于',
            })

_precompute = {
    'department_analysis': dept_results,
    'level_analysis': level_results,
    'tenure_analysis': tenure_results,
    'significant_gaps': significant_gaps,
}

with open(os.path.join(_ANALYSIS_DIR, 'step2_precompute.json'), 'w') as f:
    _json_mod.dump(_precompute, f, ensure_ascii=False, default=str)
print(_json_mod.dumps(_precompute, ensure_ascii=False, default=str, indent=2))

if all_results:
    _export_detail(_pd_mod.DataFrame(all_results), 'step2_group_comparison', '分组对比分析')
