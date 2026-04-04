# Step 0 precompute: 财务报表结构识别
# Depends on: _dfs (dict of DataFrames, injected by file preamble), _ANALYSIS_DIR
import json as _json_mod
import os as _os_mod

result = {'sheets_found': [], 'sheet_types': {}, 'periods': [], 'error': None}

try:
    if '_dfs' in dir() and _dfs:
        detected = _detect_financial_sheets(_dfs)
        result['sheets_found'] = list(_dfs.keys())
        result['sheet_types'] = {k: v[0] for k, v in detected.items()}
        result['has_income_statement'] = 'income_statement' in detected
        result['has_balance_sheet'] = 'balance_sheet' in detected
        result['has_cash_flow'] = 'cash_flow' in detected
        result['analysis_suggestion'] = (
            '三大报表齐全，可进行完整杜邦分析' if len(detected) >= 3
            else '仅有利润表，聚焦收入成本和盈利能力分析' if 'income_statement' in detected
            else '请确认上传的报表类型'
        )
    elif '_df' in dir() and _df is not None:
        result['sheets_found'] = ['单张表格']
        result['has_income_statement'] = True
        result['analysis_suggestion'] = '检测到单张表格，将尝试识别为利润表进行分析'
    else:
        result['error'] = '未检测到有效数据，请确认文件已正确上传'
except Exception as e:
    result['error'] = str(e)

with open(_os_mod.path.join(_ANALYSIS_DIR, 'step0_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, indent=2))
