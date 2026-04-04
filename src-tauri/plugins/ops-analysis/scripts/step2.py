import json as _json_mod
import os as _os_mod

result = {}
try:
    _playbooks = _KNOWLEDGE.get('growth_playbooks', {}) if '_KNOWLEDGE' in dir() else {}

    if '_df' in dir() and _df is not None:
        df = _df.copy()
        cols = list(df.columns)

        # Detect columns
        channel_cols = [c for c in cols if any(k in c.lower() for k in ['渠道', '来源', 'channel', 'source', 'medium', 'utm'])]
        time_cols = [c for c in cols if any(k in c.lower() for k in ['日期', '时间', 'date', 'time', '月', 'month'])]
        metric_cols = [c for c in cols if any(k in c.lower() for k in [
            'gmv', 'uv', 'pv', '转化', '金额', '收入', '订单', '注册', '激活',
            'dau', 'mau', '曝光', '点击', 'revenue', 'amount', 'count',
            'impression', 'click', 'conversion'
        ])]
        user_cols = [c for c in cols if any(k in c.lower() for k in ['用户', 'user', 'uid', 'user_id', '会员'])]

        summary = {}

        # Channel ranking
        if channel_cols and metric_cols:
            cc = channel_cols[0]
            for mc in metric_cols[:3]:
                df[mc] = pd.to_numeric(df[mc], errors='coerce')
                ranking = df.groupby(cc)[mc].sum().sort_values(ascending=False)
                summary[f'channel_ranking_{mc}'] = {str(k): float(v) for k, v in ranking.head(15).items()}

        # Basic retention (cohort-style) if user + time columns exist
        retention_data = {}
        if user_cols and time_cols:
            uc, tc = user_cols[0], time_cols[0]
            df[tc] = pd.to_datetime(df[tc], errors='coerce')
            df_valid = df.dropna(subset=[tc])

            if len(df_valid) > 0:
                # First activity per user
                first_seen = df_valid.groupby(uc)[tc].min().reset_index()
                first_seen.columns = [uc, 'first_date']
                first_seen['cohort'] = first_seen['first_date'].dt.to_period('M')

                merged = df_valid.merge(first_seen[[uc, 'cohort']], on=uc)
                merged['activity_month'] = merged[tc].dt.to_period('M')

                cohort_size = first_seen.groupby('cohort')[uc].nunique()
                activity = merged.groupby(['cohort', 'activity_month'])[uc].nunique().reset_index()
                activity.columns = ['cohort', 'activity_month', 'active_users']

                # Compute period offset
                activity['period_offset'] = (activity['activity_month'] - activity['cohort']).apply(lambda x: x.n if hasattr(x, 'n') else 0)
                pivot = activity.pivot_table(index='cohort', columns='period_offset', values='active_users')
                # Normalize by cohort size
                for cohort in pivot.index:
                    size = cohort_size.get(cohort, 1)
                    if size > 0:
                        pivot.loc[cohort] = pivot.loc[cohort] / size

                retention_data = {
                    'cohort_retention': {str(k): {str(c): round(float(v), 4) for c, v in row.items() if pd.notna(v)} for k, row in pivot.iterrows()},
                    'cohort_sizes': {str(k): int(v) for k, v in cohort_size.items()},
                }

        summary['retention_data'] = retention_data

        if _playbooks:
            summary['growth_playbooks'] = _playbooks

        result = summary
    else:
        result = {'error': '未找到数据'}
except Exception as e:
    result = {'error': str(e)}

_cache_result('ops_step2', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step2_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, default=str, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, default=str, indent=2))
