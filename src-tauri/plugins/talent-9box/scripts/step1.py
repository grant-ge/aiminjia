# Step 1: Performance/Potential score normalization
# Executed automatically by Rust before LLM starts.
# Depends on: _df, _ANALYSIS_DIR, _export_detail

import json as _json_mod
import pandas as _pd_mod

# --- Field detection ---
_talent_field_patterns = {
    'name': ['姓名', '员工姓名', 'name', 'employee_name'],
    'department': ['部门', 'department', 'dept'],
    'position': ['岗位', '职位', 'position', 'job_title'],
    'level': ['职级', '层级', 'level', 'grade', 'rank'],
    'age': ['年龄', 'age'],
    'tenure': ['司龄', '工龄', 'tenure', 'years_of_service', 'seniority'],
    'hire_date': ['入职日期', 'hire_date', 'start_date'],
    'performance_score': ['绩效分', '绩效得分', '绩效评分', '绩效成绩', 'performance_score', 'performance', 'perf_score', '绩效'],
    'potential_score': ['潜力分', '潜力得分', '潜力评分', 'potential_score', 'potential', 'pot_score', '潜力'],
    'performance_level': ['绩效等级', '绩效评级', 'performance_level', 'performance_rating', 'perf_rating'],
    'potential_level': ['潜力等级', '潜力评级', 'potential_level', 'potential_rating', 'pot_rating'],
}

detected = {}
col_lower = {c: c.lower().strip() for c in _df.columns}
for semantic, patterns in _talent_field_patterns.items():
    for col, col_l in col_lower.items():
        for p in patterns:
            if p.lower() in col_l or col_l in p.lower():
                detected[semantic] = col
                break
        if semantic in detected:
            break

# --- Determine performance and potential columns ---
perf_col = detected.get('performance_score') or detected.get('performance_level')
pot_col = detected.get('potential_score') or detected.get('potential_level')

# --- Normalize to numeric if needed ---
def _to_numeric_series(series):
    """Convert series to numeric, handling level/grade text"""
    numeric = _pd_mod.to_numeric(series, errors='coerce')
    if numeric.notna().sum() > 0:
        return numeric
    # Try mapping text levels
    level_map = {
        'A': 3, 'B': 2, 'C': 1, 'D': 0,
        '优': 3, '良': 2, '中': 1.5, '差': 1, '不合格': 0,
        '优秀': 3, '良好': 2, '合格': 1, '待改进': 0,
        '高': 3, '中': 2, '低': 1,
        'high': 3, 'medium': 2, 'low': 1,
        'excellent': 3, 'good': 2, 'average': 1, 'poor': 0,
    }
    mapped = series.astype(str).str.strip().map(level_map)
    return mapped

perf_numeric = _to_numeric_series(_df[perf_col]) if perf_col else None
pot_numeric = _to_numeric_series(_df[pot_col]) if pot_col else None

# --- Normalize to 1-3 scale using tertile split ---
def _normalize_to_3(series):
    """Normalize numeric series to 1/2/3 using tertile quantiles"""
    valid = series.dropna()
    if len(valid) == 0:
        return series, {}
    try:
        q33 = float(valid.quantile(0.33))
        q66 = float(valid.quantile(0.66))
    except Exception:
        q33 = float(valid.median()) - 0.5
        q66 = float(valid.median()) + 0.5
    result = series.copy()
    result = result.apply(lambda x: 1 if x <= q33 else (2 if x <= q66 else 3) if _pd_mod.notna(x) else x)
    thresholds = {'low_max': q33, 'mid_max': q66}
    return result, thresholds

perf_stats = {}
pot_stats = {}
perf_3level = None
pot_3level = None
perf_thresholds = {}
pot_thresholds = {}

if perf_numeric is not None:
    valid_perf = perf_numeric.dropna()
    if len(valid_perf) > 0:
        perf_stats = {
            'mean': round(float(valid_perf.mean()), 2),
            'median': round(float(valid_perf.median()), 2),
            'std': round(float(valid_perf.std()), 2) if len(valid_perf) > 1 else 0,
            'min': float(valid_perf.min()),
            'max': float(valid_perf.max()),
            'count': int(len(valid_perf)),
            'missing': int(perf_numeric.isna().sum()),
        }
        perf_3level, perf_thresholds = _normalize_to_3(perf_numeric)

if pot_numeric is not None:
    valid_pot = pot_numeric.dropna()
    if len(valid_pot) > 0:
        pot_stats = {
            'mean': round(float(valid_pot.mean()), 2),
            'median': round(float(valid_pot.median()), 2),
            'std': round(float(valid_pot.std()), 2) if len(valid_pot) > 1 else 0,
            'min': float(valid_pot.min()),
            'max': float(valid_pot.max()),
            'count': int(len(valid_pot)),
            'missing': int(pot_numeric.isna().sum()),
        }
        pot_3level, pot_thresholds = _normalize_to_3(pot_numeric)

# --- Count in each level ---
perf_distribution = {}
pot_distribution = {}
if perf_3level is not None:
    for level in [1, 2, 3]:
        label = {1: '低绩效', 2: '中绩效', 3: '高绩效'}[level]
        perf_distribution[label] = int((perf_3level == level).sum())
if pot_3level is not None:
    for level in [1, 2, 3]:
        label = {1: '低潜力', 2: '中潜力', 3: '高潜力'}[level]
        pot_distribution[label] = int((pot_3level == level).sum())

# Store normalized columns back to _df for export
if perf_3level is not None:
    _df['_perf_level'] = perf_3level
if pot_3level is not None:
    _df['_pot_level'] = pot_3level

# --- Build field mapping ---
field_mapping = []
semantic_zh = {
    'name': '员工姓名', 'department': '部门', 'position': '岗位',
    'level': '职级', 'age': '年龄', 'tenure': '司龄',
    'hire_date': '入职日期', 'performance_score': '绩效分数',
    'potential_score': '潜力分数', 'performance_level': '绩效等级',
    'potential_level': '潜力等级',
}
for sem, col in detected.items():
    field_mapping.append({'semantic': sem, 'semantic_zh': semantic_zh.get(sem, sem), 'column': col})

_precompute = {
    'field_mapping': field_mapping,
    'perf_col': perf_col,
    'pot_col': pot_col,
    'performance': {
        'column': perf_col,
        'stats': perf_stats,
        'thresholds': perf_thresholds,
        'distribution': perf_distribution,
    },
    'potential': {
        'column': pot_col,
        'stats': pot_stats,
        'thresholds': pot_thresholds,
        'distribution': pot_distribution,
    },
    'total_employees': len(_df),
}

with open(os.path.join(_ANALYSIS_DIR, 'step1_precompute.json'), 'w') as f:
    _json_mod.dump(_precompute, f, ensure_ascii=False, default=str)
print(_json_mod.dumps(_precompute, ensure_ascii=False, default=str, indent=2))

_export_detail(_df, 'step1_normalized_scores', '归一化后人才数据')
