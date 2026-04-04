# Step 1 precompute: 收入成本结构分析
# Depends on: _df or _dfs, _ANALYSIS_DIR
import json as _json_mod
import os as _os_mod

result = {}
try:
    # 优先用利润表
    df_pl = None
    if '_dfs' in dir() and _dfs:
        detected = _detect_financial_sheets(_dfs)
        if 'income_statement' in detected:
            df_pl = detected['income_statement'][1]
    if df_pl is None and '_df' in dir() and _df is not None:
        df_pl = _df

    if df_pl is not None:
        result = _step1_income_cost(df_pl)
    else:
        result = {'error': '未找到利润表数据'}
except Exception as e:
    result = {'error': str(e)}

_cache_result('finance_step1', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step1_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, default=str, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, default=str, indent=2))
