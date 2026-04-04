# Step 0 precompute: 预算结构识别
import json as _json_mod
import os as _os_mod

result = {'detection': {}, 'error': None}
try:
    if '_dfs' in dir() and _dfs:
        detected = _detect_budget_vs_actual(_dfs)
        result['sheets'] = list(_dfs.keys())
        result['has_budget'] = detected.get('budget') is not None
        result['has_actual'] = detected.get('actual') is not None
        result['budget_sheet'] = detected['budget'][0] if detected.get('budget') else None
        result['actual_sheet'] = detected['actual'][0] if detected.get('actual') else None
        result['source'] = detected.get('source', 'unknown')
        result['suggestion'] = (
            '预算表和实际表均已识别，可进行预实对比分析' if result['has_budget'] and result['has_actual']
            else '仅找到一张表，请确认是否需要上传预算或实际数据'
        )
    elif '_df' in dir() and _df is not None:
        result['sheets'] = ['单张表格']
        result['suggestion'] = '检测到单张表格，请告知是预算表还是实际表，或是否包含预实对比数据'
    else:
        result['error'] = '未检测到有效数据'
except Exception as e:
    result['error'] = str(e)

with open(_os_mod.path.join(_ANALYSIS_DIR, 'step0_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, indent=2))
