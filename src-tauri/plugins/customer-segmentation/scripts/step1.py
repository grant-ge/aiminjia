import json as _json_mod
import os as _os_mod
import numpy as _np

result = {}
try:
    _models = _KNOWLEDGE.get('segmentation_models', {}) if '_KNOWLEDGE' in dir() else {}

    if '_df' in dir() and _df is not None:
        df = _df.copy()
        cols = list(df.columns)

        # Auto-detect columns
        cid_cols = [c for c in cols if any(k in c.lower() for k in ['客户id', '用户id', '会员id', 'customer_id', 'user_id'])]
        amount_cols = [c for c in cols if any(k in c.lower() for k in ['金额', '消费', 'amount', 'revenue', 'spend'])]
        time_cols = [c for c in cols if any(k in c.lower() for k in ['日期', '时间', 'date', 'time'])]

        if cid_cols and amount_cols and time_cols:
            cid, ac, tc = cid_cols[0], amount_cols[0], time_cols[0]
            df[tc] = pd.to_datetime(df[tc], errors='coerce')
            df[ac] = pd.to_numeric(df[ac], errors='coerce')

            now = df[tc].max()
            rfm = df.groupby(cid).agg(
                recency=(tc, lambda x: (now - x.max()).days),
                frequency=(ac, 'count'),
                monetary=(ac, 'sum')
            ).reset_index()

            # Score 1-5
            for col in ['recency', 'frequency', 'monetary']:
                try:
                    rfm[f'{col}_score'] = pd.qcut(rfm[col], 5, labels=[5,4,3,2,1] if col == 'recency' else [1,2,3,4,5], duplicates='drop').astype(int)
                except:
                    rfm[f'{col}_score'] = 3

            rfm['rfm_score'] = rfm['recency_score'] * 100 + rfm['frequency_score'] * 10 + rfm['monetary_score']

            # Label mapping
            labels = _models.get('rfm_labels', {})
            def label_customer(r, f, m):
                if r >= 4 and f >= 4 and m >= 4: return '重要价值客户'
                if r >= 4 and f >= 1 and m >= 4: return '重要发展客户'
                if r <= 2 and f >= 4 and m >= 4: return '重要保持客户'
                if r <= 2 and f <= 2 and m >= 4: return '重要挽留客户'
                if r >= 4 and f >= 4: return '一般价值客户'
                if r >= 4: return '一般发展客户'
                if f >= 4: return '一般保持客户'
                return '一般挽留客户'

            rfm['segment'] = rfm.apply(lambda x: label_customer(x['recency_score'], x['frequency_score'], x['monetary_score']), axis=1)

            seg_dist = rfm['segment'].value_counts().to_dict()
            seg_stats = {}
            for seg in rfm['segment'].unique():
                seg_data = rfm[rfm['segment'] == seg]
                seg_stats[seg] = {
                    'count': int(len(seg_data)),
                    'pct': round(len(seg_data) / len(rfm), 3),
                    'avg_monetary': round(float(seg_data['monetary'].mean()), 2),
                    'avg_frequency': round(float(seg_data['frequency'].mean()), 1),
                    'avg_recency': round(float(seg_data['recency'].mean()), 1),
                }

            result = {
                'total_customers': len(rfm),
                'segment_distribution': seg_dist,
                'segment_stats': seg_stats,
                'rfm_summary': {
                    'recency': {'mean': round(float(rfm['recency'].mean()), 1), 'median': round(float(rfm['recency'].median()), 1)},
                    'frequency': {'mean': round(float(rfm['frequency'].mean()), 1), 'median': round(float(rfm['frequency'].median()), 1)},
                    'monetary': {'mean': round(float(rfm['monetary'].mean()), 2), 'median': round(float(rfm['monetary'].median()), 2)},
                }
            }
        else:
            result = {'error': '缺少必要字段（客户ID + 消费金额 + 日期），无法执行RFM分析', 'available_columns': cols}
    else:
        result = {'error': '未找到数据'}
except Exception as e:
    result = {'error': str(e)}

_cache_result('seg_step1', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step1_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, default=str, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, default=str, indent=2))
