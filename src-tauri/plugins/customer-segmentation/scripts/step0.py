import json as _json_mod
import os as _os_mod

result = {}
try:
    if '_df' in dir() and _df is not None:
        cols = list(_df.columns)
        row_count = len(_df)

        # Detect customer-relevant fields
        customer_id_cols = [c for c in cols if any(k in c.lower() for k in ['客户id', '用户id', '会员id', 'customer_id', 'user_id', 'member_id'])]
        amount_cols = [c for c in cols if any(k in c.lower() for k in ['金额', '消费', '订单额', 'amount', 'revenue', 'spend'])]
        time_cols = [c for c in cols if any(k in c.lower() for k in ['日期', '时间', 'date', 'time'])]
        freq_cols = [c for c in cols if any(k in c.lower() for k in ['次数', '频次', '订单数', 'frequency', 'count', 'orders'])]

        # Check RFM feasibility
        rfm_ready = bool(customer_id_cols and (amount_cols or freq_cols) and time_cols)

        result = {
            'row_count': row_count,
            'columns': cols,
            'customer_id_column': customer_id_cols[0] if customer_id_cols else None,
            'amount_column': amount_cols[0] if amount_cols else None,
            'time_column': time_cols[0] if time_cols else None,
            'frequency_column': freq_cols[0] if freq_cols else None,
            'rfm_ready': rfm_ready,
            'unique_customers': int(_df[customer_id_cols[0]].nunique()) if customer_id_cols else None,
        }
    else:
        result = {'error': '未找到数据文件'}
except Exception as e:
    result = {'error': str(e)}

_cache_result('seg_step0', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step0_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, default=str, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, default=str, indent=2))
