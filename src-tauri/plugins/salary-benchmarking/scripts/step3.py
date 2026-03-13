# Step 3: Problem diagnosis - internal equity + external competitiveness
# Depends on: _df, _ANALYSIS_DIR, _export_detail

import json as _json_mod
import pandas as _pd_mod

try:
    step1_path = os.path.join(_ANALYSIS_DIR, 'step1_precompute.json')
    with open(step1_path, 'r') as f:
        step1 = _json_mod.load(f)
except (FileNotFoundError, _json_mod.JSONDecodeError):
    step1 = {'field_mapping': []}

detected = {item['semantic']: item['column'] for item in step1.get('field_mapping', [])}
salary_col = step1.get('salary_column')
salary_data = _pd_mod.to_numeric(_df[salary_col], errors='coerce') if salary_col else None

# --- CR (Compa-Ratio) analysis ---
cr_analysis = []
if salary_data is not None:
    group_col = detected.get('position') or detected.get('level')
    if group_col:
        for grp in _df[group_col].dropna().unique():
            grp_mask = _df[group_col] == grp
            grp_salary = salary_data[grp_mask].dropna()
            if len(grp_salary) < 2:
                continue
            median = float(grp_salary.median())
            if median > 0:
                crs = grp_salary / median
                cr_analysis.append({
                    'group': str(grp),
                    'count': int(len(grp_salary)),
                    'median_salary': round(median, 0),
                    'cr_mean': round(float(crs.mean()), 3),
                    'cr_min': round(float(crs.min()), 3),
                    'cr_max': round(float(crs.max()), 3),
                    'cr_std': round(float(crs.std()), 3),
                    'spread': round(float((crs.max() - crs.min())), 3),
                })

# --- New vs Old employee inversion detection ---
inversion_cases = []
if 'tenure' in detected and salary_data is not None:
    tenure_col = detected['tenure']
    tenure_numeric = _pd_mod.to_numeric(_df[tenure_col], errors='coerce')
    group_col = detected.get('position') or detected.get('level')
    if group_col and tenure_numeric.notna().sum() > 0:
        for grp in _df[group_col].dropna().unique():
            grp_mask = _df[group_col] == grp
            grp_df = _df[grp_mask].copy()
            grp_df['_salary'] = salary_data[grp_mask]
            grp_df['_tenure'] = tenure_numeric[grp_mask]
            grp_df = grp_df.dropna(subset=['_salary', '_tenure'])
            if len(grp_df) < 4:
                continue
            new_emps = grp_df[grp_df['_tenure'] <= 1]['_salary']
            old_emps = grp_df[grp_df['_tenure'] >= 3]['_salary']
            if len(new_emps) >= 2 and len(old_emps) >= 2:
                if float(new_emps.median()) > float(old_emps.median()):
                    inversion_cases.append({
                        'group': str(grp),
                        'new_median': round(float(new_emps.median()), 0),
                        'old_median': round(float(old_emps.median()), 0),
                        'gap_pct': round((float(new_emps.median()) - float(old_emps.median())) / float(old_emps.median()) * 100, 1),
                    })

# --- Red/Green circle detection ---
red_circle = []
green_circle = []
if salary_data is not None:
    group_col = detected.get('level') or detected.get('position')
    if group_col:
        for grp in _df[group_col].dropna().unique():
            grp_salary = salary_data[_df[group_col] == grp].dropna()
            if len(grp_salary) < 3:
                continue
            p75 = float(grp_salary.quantile(0.75))
            p25 = float(grp_salary.quantile(0.25))
            iqr = p75 - p25
            upper = p75 + 1.5 * iqr
            lower = max(p25 - 1.5 * iqr, 0)
            red = int((grp_salary > upper).sum())
            green = int((grp_salary < lower).sum())
            if red > 0:
                red_circle.append({'group': str(grp), 'count': red, 'threshold': round(upper, 0)})
            if green > 0:
                green_circle.append({'group': str(grp), 'count': green, 'threshold': round(lower, 0)})

_precompute = {
    'cr_analysis': cr_analysis,
    'inversion_cases': inversion_cases,
    'red_circle': red_circle,
    'green_circle': green_circle,
}

with open(os.path.join(_ANALYSIS_DIR, 'step3_precompute.json'), 'w') as f:
    _json_mod.dump(_precompute, f, ensure_ascii=False, default=str)
print(_json_mod.dumps(_precompute, ensure_ascii=False, default=str, indent=2))

# Export diagnosis details
rows = []
for item in cr_analysis:
    rows.append({**item, 'type': 'CR分析'})
for item in inversion_cases:
    rows.append({**item, 'type': '新老倒挂'})
for item in red_circle:
    rows.append({**item, 'type': '红圈员工'})
for item in green_circle:
    rows.append({**item, 'type': '绿圈员工'})
if rows:
    _export_detail(_pd_mod.DataFrame(rows), 'step3_diagnosis', '薪酬诊断明细')
