import json as _json_mod
import os as _os_mod

result = {}
try:
    if '_df' in dir() and _df is not None:
        cols = list(_df.columns)
        dtypes = {c: str(_df[c].dtype) for c in cols}
        row_count = len(_df)

        # Detect business model
        biz_type = 'general'
        col_lower = [c.lower() for c in cols]
        col_text = ' '.join(col_lower)
        if any(k in col_text for k in ['mrr', 'arr', 'churn', 'subscription']):
            biz_type = 'saas'
        elif any(k in col_text for k in ['gmv', 'uv', 'pv', '转化', 'conversion']):
            biz_type = 'ecommerce'
        elif any(k in col_text for k in ['赢单', 'pipeline', '商机', 'deal', 'opportunity']):
            biz_type = 'b2b'

        # Detect key fields
        time_cols = [c for c in cols if any(k in c.lower() for k in ['日期', '时间', 'date', 'time', '月', 'month'])]
        amount_cols = [c for c in cols if any(k in c.lower() for k in ['金额', '收入', '销售额', 'amount', 'revenue', 'gmv', 'sales'])]

        result = {
            'row_count': row_count,
            'columns': cols,
            'dtypes': dtypes,
            'detected_biz_type': biz_type,
            'time_columns': time_cols,
            'amount_columns': amount_cols,
            'sample_values': {c: str(_df[c].iloc[0]) if len(_df) > 0 else '' for c in cols[:10]}
        }
    else:
        result = {'error': '未找到数据文件'}
except Exception as e:
    result = {'error': str(e)}

_cache_result('sales_step0', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step0_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, default=str, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, default=str, indent=2))
