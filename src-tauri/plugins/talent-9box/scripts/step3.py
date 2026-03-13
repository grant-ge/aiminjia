# Step 3: Talent structure analysis by department, age, tenure
# Executed automatically by Rust before LLM starts.
# Depends on: _df, _ANALYSIS_DIR, _export_detail

import json as _json_mod
import pandas as _pd_mod

# Load step1 cache for field mapping
try:
    step1_path = os.path.join(_ANALYSIS_DIR, 'step1_precompute.json')
    with open(step1_path, 'r') as f:
        step1 = _json_mod.load(f)
except (FileNotFoundError, _json_mod.JSONDecodeError):
    step1 = {}

detected = {item['semantic']: item['column'] for item in step1.get('field_mapping', [])}

# Recompute 9-box labels if not present (columns don't persist across steps)
if '_9box_label' not in _df.columns:
    perf_col = step1.get('perf_col')
    pot_col = step1.get('pot_col')
    perf_thresholds = step1.get('performance', {}).get('thresholds', {})
    pot_thresholds = step1.get('potential', {}).get('thresholds', {})

    _9box_labels = {
        (3, 3): '明星人才', (3, 2): '核心骨干', (3, 1): '专业专家',
        (2, 3): '高潜新星', (2, 2): '稳定贡献者', (2, 1): '待发展者',
        (1, 3): '待激活者', (1, 2): '观察对象', (1, 1): '需改进者',
    }

    if perf_col and pot_col and perf_col in _df.columns and pot_col in _df.columns:
        def _to_numeric_fallback(series):
            numeric = _pd_mod.to_numeric(series, errors='coerce')
            if numeric.notna().sum() > 0:
                return numeric
            level_map = {
                'A': 3, 'B': 2, 'C': 1, 'D': 0,
                '优': 3, '良': 2, '中': 1.5, '差': 1, '不合格': 0,
                '优秀': 3, '良好': 2, '合格': 1, '待改进': 0,
                '高': 3, '中': 2, '低': 1, 'high': 3, 'medium': 2, 'low': 1,
            }
            return series.astype(str).str.strip().map(level_map)

        def _apply_3level(series, thresholds):
            if not thresholds:
                valid = series.dropna()
                if len(valid) == 0:
                    return series
                q33 = float(valid.quantile(0.33))
                q66 = float(valid.quantile(0.66))
            else:
                valid = series.dropna()
                if len(valid) == 0:
                    return series
                q33 = thresholds.get('low_max', float(valid.quantile(0.33)))
                q66 = thresholds.get('mid_max', float(valid.quantile(0.66)))
            return series.apply(lambda x: 1 if x <= q33 else (2 if x <= q66 else 3) if _pd_mod.notna(x) else x)

        perf_numeric = _to_numeric_fallback(_df[perf_col])
        pot_numeric = _to_numeric_fallback(_df[pot_col])
        _df['_perf_level'] = _apply_3level(perf_numeric, perf_thresholds)
        _df['_pot_level'] = _apply_3level(pot_numeric, pot_thresholds)

        def _assign_label(row):
            if _pd_mod.notna(row.get('_perf_level')) and _pd_mod.notna(row.get('_pot_level')):
                try:
                    return _9box_labels.get((int(row['_perf_level']), int(row['_pot_level'])), '未分类')
                except (ValueError, TypeError):
                    return '数据异常'
            return '数据缺失'

        _df['_9box_label'] = _df.apply(_assign_label, axis=1)

# --- Department breakdown ---
dept_analysis = {}
if 'department' in detected and '_9box_label' in _df.columns:
    dept_col = detected['department']
    for dept in _df[dept_col].dropna().unique():
        dept_df = _df[_df[dept_col] == dept]
        dist = dept_df['_9box_label'].value_counts().to_dict()
        dept_analysis[str(dept)] = {
            'total': len(dept_df),
            'distribution': {k: int(v) for k, v in dist.items()},
            'star_count': int(dist.get('明星人才', 0)),
            'risk_count': int(sum(dist.get(l, 0) for l in ['需改进者', '观察对象', '待激活者'])),
        }

# --- Age breakdown ---
age_analysis = {}
if 'age' in detected:
    age_col = detected['age']
    age_numeric = _pd_mod.to_numeric(_df[age_col], errors='coerce')
    if age_numeric.notna().sum() > 0:
        bins = [0, 25, 30, 35, 40, 50, 100]
        labels = ['25岁以下', '25-30岁', '30-35岁', '35-40岁', '40-50岁', '50岁以上']
        _df['_age_group'] = _pd_mod.cut(age_numeric, bins=bins, labels=labels, right=False)
        for grp in labels:
            grp_df = _df[_df['_age_group'] == grp]
            if len(grp_df) > 0 and '_9box_label' in grp_df.columns:
                dist = grp_df['_9box_label'].value_counts().to_dict()
                age_analysis[grp] = {
                    'total': len(grp_df),
                    'distribution': {k: int(v) for k, v in dist.items()},
                }

# --- Tenure breakdown ---
tenure_analysis = {}
if 'tenure' in detected:
    tenure_col = detected['tenure']
    tenure_numeric = _pd_mod.to_numeric(_df[tenure_col], errors='coerce')
    if tenure_numeric.notna().sum() > 0:
        bins = [0, 1, 3, 5, 10, 100]
        labels = ['1年以下', '1-3年', '3-5年', '5-10年', '10年以上']
        _df['_tenure_group'] = _pd_mod.cut(tenure_numeric, bins=bins, labels=labels, right=False)
        for grp in labels:
            grp_df = _df[_df['_tenure_group'] == grp]
            if len(grp_df) > 0 and '_9box_label' in grp_df.columns:
                dist = grp_df['_9box_label'].value_counts().to_dict()
                tenure_analysis[grp] = {
                    'total': len(grp_df),
                    'distribution': {k: int(v) for k, v in dist.items()},
                }

_precompute = {
    'department_analysis': dept_analysis,
    'age_analysis': age_analysis,
    'tenure_analysis': tenure_analysis,
}

with open(os.path.join(_ANALYSIS_DIR, 'step3_precompute.json'), 'w') as f:
    _json_mod.dump(_precompute, f, ensure_ascii=False, default=str)
print(_json_mod.dumps(_precompute, ensure_ascii=False, default=str, indent=2))

# Auto-export
rows = []
for dept, data in dept_analysis.items():
    for label, count in data['distribution'].items():
        rows.append({'部门': dept, '九宫格位置': label, '人数': count})
if rows:
    _export_detail(_pd_mod.DataFrame(rows), 'step3_structure_analysis', '人才结构分析明细')
