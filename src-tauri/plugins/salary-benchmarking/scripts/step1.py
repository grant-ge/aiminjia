# Step 1: Internal salary structure analysis
# Depends on: _df, _ANALYSIS_DIR, _export_detail

import json as _json_mod
import pandas as _pd_mod

# --- Field detection ---
_salary_field_patterns = {
    'name': ['姓名', '员工姓名', 'name', 'employee_name'],
    'department': ['部门', 'department', 'dept'],
    'position': ['岗位', '职位', 'position', 'job_title', 'role'],
    'level': ['职级', '层级', 'level', 'grade', 'rank', '薪酬等级'],
    'tenure': ['司龄', '工龄', 'tenure', 'years_of_service'],
    'hire_date': ['入职日期', 'hire_date', 'start_date'],
    'base_salary': ['基本工资', '基本薪资', '固定工资', 'base_salary', 'base_pay', '月薪'],
    'total_cash': ['总现金', '年薪', '年度总薪酬', 'total_cash', 'annual_salary', '应发工资'],
    'bonus': ['奖金', '绩效奖金', '年终奖', 'bonus', 'incentive'],
    'allowance': ['补贴', '津贴', 'allowance'],
    'market_p50': ['市场中位', '市场P50', 'market_p50', 'market_median'],
    'market_p25': ['市场P25', 'market_p25'],
    'market_p75': ['市场P75', 'market_p75'],
}

detected = {}
col_lower = {c: c.lower().strip() for c in _df.columns}
for semantic, patterns in _salary_field_patterns.items():
    for col, col_l in col_lower.items():
        for p in patterns:
            if p.lower() in col_l or col_l in p.lower():
                detected[semantic] = col
                break
        if semantic in detected:
            break

# --- Determine primary salary column ---
salary_col = detected.get('base_salary') or detected.get('total_cash')
salary_data = _pd_mod.to_numeric(_df[salary_col], errors='coerce') if salary_col else None

# --- Overall distribution ---
overall_stats = {}
if salary_data is not None:
    valid = salary_data.dropna()
    if len(valid) > 0:
        overall_stats = {
            'count': int(len(valid)),
            'mean': round(float(valid.mean()), 0),
            'median': round(float(valid.median()), 0),
            'p25': round(float(valid.quantile(0.25)), 0),
            'p50': round(float(valid.quantile(0.50)), 0),
            'p75': round(float(valid.quantile(0.75)), 0),
            'p90': round(float(valid.quantile(0.90)), 0),
            'min': round(float(valid.min()), 0),
            'max': round(float(valid.max()), 0),
            'std': round(float(valid.std()), 0) if len(valid) > 1 else 0,
        }

# --- Fixed/Variable ratio ---
fixed_variable = None
if 'base_salary' in detected and ('bonus' in detected or 'total_cash' in detected):
    base = _pd_mod.to_numeric(_df[detected['base_salary']], errors='coerce')
    if 'total_cash' in detected:
        total = _pd_mod.to_numeric(_df[detected['total_cash']], errors='coerce')
    elif 'bonus' in detected:
        bonus = _pd_mod.to_numeric(_df[detected['bonus']], errors='coerce').fillna(0)
        total = base + bonus
    else:
        total = base
    valid = (base.notna()) & (total.notna()) & (total > 0)
    if valid.sum() > 0:
        ratio = (base[valid] / total[valid]).mean()
        fixed_variable = {
            'fixed_ratio': round(float(ratio * 100), 1),
            'variable_ratio': round(float((1 - ratio) * 100), 1),
        }

# --- Department/Level breakdown ---
dept_stats = []
if 'department' in detected and salary_data is not None:
    dept_col = detected['department']
    for dept in _df[dept_col].dropna().unique():
        dept_salaries = salary_data[_df[dept_col] == dept].dropna()
        if len(dept_salaries) >= 3:
            dept_stats.append({
                'department': str(dept),
                'count': int(len(dept_salaries)),
                'p25': round(float(dept_salaries.quantile(0.25)), 0),
                'p50': round(float(dept_salaries.quantile(0.50)), 0),
                'p75': round(float(dept_salaries.quantile(0.75)), 0),
                'mean': round(float(dept_salaries.mean()), 0),
            })

level_stats = []
if 'level' in detected and salary_data is not None:
    level_col = detected['level']
    for lvl in sorted(_df[level_col].dropna().unique(), key=str):
        lvl_salaries = salary_data[_df[level_col] == lvl].dropna()
        if len(lvl_salaries) >= 2:
            level_stats.append({
                'level': str(lvl),
                'count': int(len(lvl_salaries)),
                'min': round(float(lvl_salaries.min()), 0),
                'p50': round(float(lvl_salaries.median()), 0),
                'max': round(float(lvl_salaries.max()), 0),
                'bandwidth': round(float((lvl_salaries.max() - lvl_salaries.min()) / lvl_salaries.min() * 100), 1) if lvl_salaries.min() > 0 else 0,
            })

# --- Build field mapping ---
field_mapping = []
semantic_zh = {
    'name': '员工姓名', 'department': '部门', 'position': '岗位',
    'level': '职级', 'tenure': '司龄', 'hire_date': '入职日期',
    'base_salary': '基本工资', 'total_cash': '总现金薪酬',
    'bonus': '奖金', 'allowance': '补贴',
    'market_p50': '市场中位值', 'market_p25': '市场P25', 'market_p75': '市场P75',
}
for sem, col in detected.items():
    field_mapping.append({'semantic': sem, 'semantic_zh': semantic_zh.get(sem, sem), 'column': col})

has_market_data = any(k.startswith('market_') for k in detected)

_precompute = {
    'field_mapping': field_mapping,
    'salary_column': salary_col,
    'overall_stats': overall_stats,
    'fixed_variable_ratio': fixed_variable,
    'department_stats': dept_stats,
    'level_stats': level_stats,
    'has_market_data': has_market_data,
    'total_employees': len(_df),
}

with open(os.path.join(_ANALYSIS_DIR, 'step1_precompute.json'), 'w') as f:
    _json_mod.dump(_precompute, f, ensure_ascii=False, default=str)
print(_json_mod.dumps(_precompute, ensure_ascii=False, default=str, indent=2))

_export_detail(_df, 'step1_salary_structure', '内部薪酬结构数据')
