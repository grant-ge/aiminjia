import json as _json_mod
import os as _os_mod

result = {}
try:
    _benchmarks = _KNOWLEDGE.get('benchmarks', {}) if '_KNOWLEDGE' in dir() else {}
    _playbooks = _KNOWLEDGE.get('retention_playbooks', {}) if '_KNOWLEDGE' in dir() else {}

    if '_df' in dir() and _df is not None:
        df = _df.copy()
        cols = list(df.columns)
        cid_cols = [c for c in cols if any(k in c.lower() for k in ['客户id', '用户id', '会员id', 'customer_id', 'user_id'])]
        amount_cols = [c for c in cols if any(k in c.lower() for k in ['金额', '消费', 'amount', 'revenue', 'spend'])]
        time_cols = [c for c in cols if any(k in c.lower() for k in ['日期', '时间', 'date', 'time'])]

        summary = {}
        if cid_cols and amount_cols and time_cols:
            cid, ac, tc = cid_cols[0], amount_cols[0], time_cols[0]
            df[tc] = pd.to_datetime(df[tc], errors='coerce')
            df[ac] = pd.to_numeric(df[ac], errors='coerce')

            # Customer lifetime value estimate
            cust = df.groupby(cid).agg(
                total_spend=(ac, 'sum'),
                order_count=(ac, 'count'),
                first_order=(tc, 'min'),
                last_order=(tc, 'max')
            )
            cust['lifespan_days'] = (cust['last_order'] - cust['first_order']).dt.days
            cust['avg_order_value'] = cust['total_spend'] / cust['order_count']

            summary['ltv_stats'] = {
                'mean': round(float(cust['total_spend'].mean()), 2),
                'median': round(float(cust['total_spend'].median()), 2),
                'top10_pct_ltv': round(float(cust['total_spend'].quantile(0.9)), 2),
            }

            # Lifecycle segmentation
            now = df[tc].max()
            cust['days_since_last'] = (now - cust['last_order']).dt.days
            cust['lifecycle'] = cust['days_since_last'].apply(
                lambda d: '活跃' if d <= 30 else '沉默' if d <= 90 else '流失预警' if d <= 180 else '已流失'
            )
            lifecycle_dist = cust['lifecycle'].value_counts().to_dict()
            summary['lifecycle_distribution'] = lifecycle_dist
            summary['churn_risk_count'] = int((cust['lifecycle'].isin(['流失预警', '已流失'])).sum())
            summary['churn_risk_pct'] = round(summary['churn_risk_count'] / len(cust), 3)

        result = summary
        if _benchmarks:
            result['industry_benchmarks'] = _benchmarks
        if _playbooks:
            result['retention_playbooks'] = _playbooks
    else:
        result = {'error': '未找到数据'}
except Exception as e:
    result = {'error': str(e)}

_cache_result('seg_step2', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step2_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, default=str, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, default=str, indent=2))
