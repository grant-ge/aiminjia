import json as _json_mod
import os as _os_mod

result = {}
try:
    _playbooks = _KNOWLEDGE.get('engagement_playbooks', {}) if '_KNOWLEDGE' in dir() else {}

    if '_df' in dir() and _df is not None:
        df = _df.copy()
        cols = list(df.columns)

        # Detect columns
        user_id_col = None
        for c in cols:
            if any(k in c.lower() for k in ['user_id', 'userid', '用户id', 'uid', '用户', 'member_id']):
                user_id_col = c
                break

        event_col = None
        for c in cols:
            if any(k in c.lower() for k in ['event', 'action', '事件', '行为', '操作', 'event_name', 'event_type']):
                event_col = c
                break

        time_col = None
        for c in cols:
            if any(k in c.lower() for k in ['时间', '日期', 'time', 'date', 'timestamp', 'created_at']):
                time_col = c
                break

        summary = {}

        if event_col and user_id_col:
            total_users = df[user_id_col].nunique()

            # Feature/event frequency ranking
            feature_counts = df[event_col].value_counts()
            summary['feature_ranking'] = {str(k): int(v) for k, v in feature_counts.head(20).items()}

            # Feature penetration rate
            feature_users = df.groupby(event_col)[user_id_col].nunique()
            feature_penetration = (feature_users / total_users).sort_values(ascending=False)
            summary['feature_penetration'] = {str(k): round(float(v), 4) for k, v in feature_penetration.head(20).items()}

            # Top paths (most common event sequences of length 3)
            if time_col:
                df[time_col] = pd.to_datetime(df[time_col], errors='coerce')
                df_sorted = df.dropna(subset=[time_col]).sort_values([user_id_col, time_col])

                # Build sequences per user (limit to first 50 events per user for performance)
                path_counts = {}
                for uid, group in df_sorted.groupby(user_id_col):
                    events = group[event_col].head(50).tolist()
                    for i in range(len(events) - 2):
                        path = f"{events[i]} -> {events[i+1]} -> {events[i+2]}"
                        path_counts[path] = path_counts.get(path, 0) + 1

                # Top 10 paths
                sorted_paths = sorted(path_counts.items(), key=lambda x: x[1], reverse=True)[:10]
                summary['top_paths'] = {k: v for k, v in sorted_paths}

        if _playbooks:
            summary['engagement_playbooks'] = _playbooks

        result = summary
    else:
        result = {'error': '未找到数据'}
except Exception as e:
    result = {'error': str(e)}

_cache_result('behavior_step2', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step2_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, default=str, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, default=str, indent=2))
