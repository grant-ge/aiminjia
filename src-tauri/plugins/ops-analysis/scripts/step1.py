import json as _json_mod
import os as _os_mod

result = {}
try:
    _benchmarks = _KNOWLEDGE.get('benchmarks', {}) if '_KNOWLEDGE' in dir() else {}

    if '_df' in dir() and _df is not None:
        df = _df.copy()
        cols = list(df.columns)

        # Detect columns
        time_cols = [c for c in cols if any(k in c.lower() for k in ['日期', '时间', 'date', 'time', '月', 'month', '周', 'week'])]
        metric_cols = [c for c in cols if any(k in c.lower() for k in [
            'gmv', 'uv', 'pv', '转化', '金额', '收入', '订单', '注册', '激活',
            'dau', 'mau', '曝光', '点击', '互动', 'revenue', 'amount', 'count',
            'impression', 'click', 'conversion', '参与', '报名'
        ])]

        summary = {
            'total_rows': len(df),
            'time_column': time_cols[0] if time_cols else None,
            'metric_columns': metric_cols,
        }

        # Monthly aggregation if time column found
        if time_cols and metric_cols:
            tc = time_cols[0]
            df[tc] = pd.to_datetime(df[tc], errors='coerce')

            for mc in metric_cols:
                df[mc] = pd.to_numeric(df[mc], errors='coerce')

            agg_dict = {mc: 'sum' for mc in metric_cols}
            monthly = df.groupby(df[tc].dt.to_period('M')).agg(agg_dict).reset_index()
            monthly[tc] = monthly[tc].astype(str)

            # Compute MoM for each metric
            for mc in metric_cols:
                monthly[f'{mc}_mom'] = monthly[mc].pct_change()

            summary['monthly_trend'] = monthly.to_dict('records')

            # Metric summary
            metric_summary = {}
            for mc in metric_cols:
                metric_summary[mc] = {
                    'total': float(df[mc].sum()),
                    'mean': float(df[mc].mean()),
                    'min': float(df[mc].min()),
                    'max': float(df[mc].max()),
                }
            summary['metric_summary'] = metric_summary

        if _benchmarks:
            summary['industry_benchmarks'] = _benchmarks

        result = summary
    else:
        result = {'error': '未找到数据'}
except Exception as e:
    result = {'error': str(e)}

_cache_result('ops_step1', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step1_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, default=str, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, default=str, indent=2))
