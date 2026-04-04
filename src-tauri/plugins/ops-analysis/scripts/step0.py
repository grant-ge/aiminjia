import json as _json_mod
import os as _os_mod

result = {}
try:
    if '_df' in dir() and _df is not None:
        cols = list(_df.columns)
        dtypes = {c: str(_df[c].dtype) for c in cols}
        row_count = len(_df)
        col_lower = [c.lower() for c in cols]
        col_text = ' '.join(col_lower)

        # Detect ops scene
        detected_scene = 'general'
        if any(k in col_text for k in ['gmv', 'uv', 'pv', '转化', 'conversion', '客单价', '订单']):
            detected_scene = 'ecommerce'
        elif any(k in col_text for k in ['发布', '曝光', '互动', '阅读', '点赞', '分享', 'impression', 'engagement']):
            detected_scene = 'content'
        elif any(k in col_text for k in ['dau', 'mau', '注册', '激活', '新增', 'signup', 'activation']):
            detected_scene = 'user_growth'
        elif any(k in col_text for k in ['活动', '参与', '报名', 'campaign', 'event', '优惠券', 'coupon']):
            detected_scene = 'activity'

        # Detect key columns
        time_cols = [c for c in cols if any(k in c.lower() for k in ['日期', '时间', 'date', 'time', '月', 'month', '周', 'week'])]
        metric_cols = [c for c in cols if any(k in c.lower() for k in [
            'gmv', 'uv', 'pv', '转化', '金额', '收入', '订单', '注册', '激活',
            'dau', 'mau', '曝光', '点击', '互动', 'revenue', 'amount', 'count',
            'impression', 'click', 'conversion', '参与', '报名'
        ])]

        result = {
            'detected_scene': detected_scene,
            'metric_columns': metric_cols,
            'time_columns': time_cols,
            'row_count': row_count,
            'columns': cols,
            'dtypes': dtypes,
            'sample_values': {c: str(_df[c].iloc[0]) if len(_df) > 0 else '' for c in cols[:10]}
        }
    else:
        result = {'error': '未找到数据文件'}
except Exception as e:
    result = {'error': str(e)}

_cache_result('ops_step0', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step0_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, default=str, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, default=str, indent=2))
