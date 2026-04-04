# Step 1 precompute: 差异分析
import json as _json_mod
import os as _os_mod

result = {}
try:
    df_budget, df_actual = None, None
    if '_dfs' in dir() and _dfs:
        detected = _detect_budget_vs_actual(_dfs)
        if detected.get('budget'):
            df_budget = detected['budget'][1]
        if detected.get('actual'):
            df_actual = detected['actual'][1]
    if df_actual is None and '_df' in dir() and _df is not None:
        df_actual = _df

    if df_actual is not None:
        result = _step1_variance_analysis(df_budget, df_actual) if df_budget is not None else {'error': '未找到预算表，仅分析实际数据结构', 'actual_rows': len(df_actual), 'actual_cols': list(df_actual.columns)}
    else:
        result = {'error': '未找到实际执行数据'}
except Exception as e:
    result = {'error': str(e)}

_variance_rules = _KNOWLEDGE.get('variance_rules', {}) if '_KNOWLEDGE' in dir() else {}
if _variance_rules.get('variance_categories'):
    result['attribution_guide'] = _variance_rules['variance_categories']
if _variance_rules.get('severity_thresholds'):
    result['severity_scale'] = _variance_rules['severity_thresholds']

_cache_result('budget_step1', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step1_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, default=str, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, default=str, indent=2))
