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

        # Detect user_id column
        user_id_col = None
        for c in cols:
            if any(k in c.lower() for k in ['user_id', 'userid', '用户id', 'uid', '用户', 'member_id']):
                user_id_col = c
                break

        # Detect event column
        event_col = None
        for c in cols:
            if any(k in c.lower() for k in ['event', 'action', '事件', '行为', '操作', 'event_name', 'event_type']):
                event_col = c
                break

        # Detect time column
        time_col = None
        for c in cols:
            if any(k in c.lower() for k in ['时间', '日期', 'time', 'date', 'timestamp', 'created_at']):
                time_col = c
                break

        # Event type counts
        event_type_counts = {}
        if event_col:
            event_type_counts = _df[event_col].value_counts().head(20).to_dict()
            event_type_counts = {str(k): int(v) for k, v in event_type_counts.items()}

        # Unique users
        unique_users = int(_df[user_id_col].nunique()) if user_id_col else None

        # Date range
        date_range = {}
        if time_col:
            ts = pd.to_datetime(_df[time_col], errors='coerce')
            valid_ts = ts.dropna()
            if len(valid_ts) > 0:
                date_range = {'min': str(valid_ts.min()), 'max': str(valid_ts.max())}

        result = {
            'user_id_column': user_id_col,
            'event_column': event_col,
            'time_column': time_col,
            'event_type_counts': event_type_counts,
            'unique_users': unique_users,
            'date_range': date_range,
            'row_count': row_count,
            'columns': cols,
            'dtypes': dtypes,
            'sample_values': {c: str(_df[c].iloc[0]) if len(_df) > 0 else '' for c in cols[:10]}
        }
    else:
        result = {'error': '未找到数据文件'}
except Exception as e:
    result = {'error': str(e)}

_cache_result('behavior_step0', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step0_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, default=str, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, default=str, indent=2))
