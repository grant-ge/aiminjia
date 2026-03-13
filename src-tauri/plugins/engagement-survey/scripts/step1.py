# Step 1: Overall metrics calculation (eNPS + dimension scores)
# Executed automatically by Rust before LLM starts.
# Depends on: _df, _ANALYSIS_DIR, _export_detail

import json as _json_mod
import pandas as _pd_mod

# --- Field detection ---
_survey_field_patterns = {
    'respondent_id': ['编号', '序号', 'id', 'respondent_id', '员工编号'],
    'department': ['部门', 'department', 'dept'],
    'level': ['职级', '层级', 'level', 'grade', 'rank', '职位级别'],
    'tenure': ['司龄', '工龄', 'tenure', 'years_of_service'],
    'age': ['年龄', 'age'],
    'gender': ['性别', 'gender'],
    'enps_score': ['推荐度', 'eNPS', '推荐意愿', 'recommend', 'nps', '净推荐值'],
}

detected = {}
col_lower = {c: c.lower().strip() for c in _df.columns}
for semantic, patterns in _survey_field_patterns.items():
    for col, col_l in col_lower.items():
        for p in patterns:
            if p.lower() in col_l or col_l in p.lower():
                detected[semantic] = col
                break
        if semantic in detected:
            break

# --- Detect dimension columns ---
# Dimension columns are typically numeric columns that are not demographic fields
demo_cols = set(detected.values())
dimension_cols = []
for col in _df.columns:
    if col not in demo_cols:
        numeric = _pd_mod.to_numeric(_df[col], errors='coerce')
        if numeric.notna().sum() > len(_df) * 0.5:  # At least 50% valid
            dimension_cols.append(col)

# --- Calculate eNPS ---
enps_result = None
if 'enps_score' in detected:
    enps_col = detected['enps_score']
    scores = _pd_mod.to_numeric(_df[enps_col], errors='coerce').dropna()
    if len(scores) > 0:
        # Determine scale
        max_score = scores.max()
        if max_score <= 5:
            promoters = (scores >= 4).sum()
            detractors = (scores <= 2).sum()
        else:
            promoters = (scores >= 9).sum()
            detractors = (scores <= 6).sum()
        total = len(scores)
        enps_score = round((promoters - detractors) / total * 100, 1)
        enps_result = {
            'score': enps_score,
            'promoters': int(promoters),
            'passives': int(total - promoters - detractors),
            'detractors': int(detractors),
            'total_responses': int(total),
            'interpretation': '优秀' if enps_score > 30 else ('良好' if enps_score > 10 else ('一般' if enps_score > 0 else '需改善')),
        }

# --- Calculate dimension scores ---
dimension_scores = []
for col in dimension_cols:
    scores = _pd_mod.to_numeric(_df[col], errors='coerce').dropna()
    if len(scores) > 0:
        dimension_scores.append({
            'dimension': col,
            'mean': round(float(scores.mean()), 2),
            'median': round(float(scores.median()), 2),
            'std': round(float(scores.std()), 2) if len(scores) > 1 else 0,
            'min': float(scores.min()),
            'max': float(scores.max()),
            'response_count': int(len(scores)),
        })

# Sort by mean score
dimension_scores.sort(key=lambda x: x['mean'], reverse=True)

# --- Overall engagement score ---
if dimension_scores:
    overall_score = round(sum(d['mean'] for d in dimension_scores) / len(dimension_scores), 2)
else:
    overall_score = None

_precompute = {
    'detected_fields': {sem: col for sem, col in detected.items()},
    'dimension_columns': dimension_cols,
    'enps': enps_result,
    'dimension_scores': dimension_scores,
    'overall_engagement_score': overall_score,
    'total_responses': len(_df),
}

with open(os.path.join(_ANALYSIS_DIR, 'step1_precompute.json'), 'w') as f:
    _json_mod.dump(_precompute, f, ensure_ascii=False, default=str)
print(_json_mod.dumps(_precompute, ensure_ascii=False, default=str, indent=2))

if dimension_scores:
    _export_detail(_pd_mod.DataFrame(dimension_scores), 'step1_overall_metrics', '整体指标汇总')
