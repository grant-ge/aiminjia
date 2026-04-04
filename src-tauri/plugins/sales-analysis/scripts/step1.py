import json as _json_mod
import os as _os_mod

result = {}
try:
    _benchmarks = _KNOWLEDGE.get('benchmarks', {}) if '_KNOWLEDGE' in dir() else {}

    if '_df' in dir() and _df is not None:
        df = _df.copy()
        cols = list(df.columns)

        # Find time and amount columns from step0 or detect again
        time_cols = [c for c in cols if any(k in c.lower() for k in ['日期', '时间', 'date', 'time', '月', 'month'])]
        amount_cols = [c for c in cols if any(k in c.lower() for k in ['金额', '收入', '销售额', 'amount', 'revenue', 'gmv', 'sales'])]

        summary = {
            'total_rows': len(df),
            'time_column': time_cols[0] if time_cols else None,
            'amount_column': amount_cols[0] if amount_cols else None,
        }

        # Basic aggregation if time + amount found
        if time_cols and amount_cols:
            tc, ac = time_cols[0], amount_cols[0]
            df[tc] = pd.to_datetime(df[tc], errors='coerce')
            df[ac] = pd.to_numeric(df[ac], errors='coerce')
            monthly = df.groupby(df[tc].dt.to_period('M'))[ac].agg(['sum', 'count', 'mean']).reset_index()
            monthly[tc] = monthly[tc].astype(str)
            summary['monthly_trend'] = monthly.to_dict('records')
            summary['total_revenue'] = float(df[ac].sum())
            summary['avg_order_value'] = float(df[ac].mean())
            summary['order_count'] = int(df[ac].count())

        # Top N analysis
        cat_cols = [c for c in cols if any(k in c.lower() for k in ['产品', '品类', '区域', '渠道', '销售', 'product', 'category', 'region', 'channel', 'rep'])]
        if cat_cols and amount_cols:
            ac = amount_cols[0]
            for cc in cat_cols[:3]:
                top = df.groupby(cc)[ac].sum().sort_values(ascending=False).head(10)
                summary[f'top_{cc}'] = {str(k): float(v) for k, v in top.items()}

        result = summary

        if _benchmarks:
            result['industry_benchmarks'] = _benchmarks
    else:
        result = {'error': '未找到数据'}
except Exception as e:
    result = {'error': str(e)}

_cache_result('sales_step1', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step1_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, default=str, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, default=str, indent=2))
