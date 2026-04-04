# Step 2 precompute: 滚动预测
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
        result = _step2_rolling_forecast(df_actual, df_budget)
    else:
        result = {'error': '未找到实际执行数据，无法预测'}
except Exception as e:
    result = {'error': str(e)}

_cache_result('budget_step2', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step2_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, default=str, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, default=str, indent=2))
