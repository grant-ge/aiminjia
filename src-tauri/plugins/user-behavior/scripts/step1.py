import json as _json_mod
import os as _os_mod

result = {}
try:
    _benchmarks = _KNOWLEDGE.get('benchmarks', {}) if '_KNOWLEDGE' in dir() else {}

    if '_df' in dir() and _df is not None:
        df = _df.copy()
        cols = list(df.columns)

        # Detect columns
        user_id_col = None
        for c in cols:
            if any(k in c.lower() for k in ['user_id', 'userid', '用户id', 'uid', '用户', 'member_id']):
                user_id_col = c
                break

        time_col = None
        for c in cols:
            if any(k in c.lower() for k in ['时间', '日期', 'time', 'date', 'timestamp', 'created_at']):
                time_col = c
                break

        summary = {}

        if user_id_col and time_col:
            df[time_col] = pd.to_datetime(df[time_col], errors='coerce')
            df_valid = df.dropna(subset=[time_col])
            df_valid['_date'] = df_valid[time_col].dt.date

            # DAU trend
            dau = df_valid.groupby('_date')[user_id_col].nunique().reset_index()
            dau.columns = ['date', 'dau']
            dau['date'] = dau['date'].astype(str)
            summary['dau_trend'] = dau.tail(60).to_dict('records')

            # WAU / MAU
            df_valid['_week'] = df_valid[time_col].dt.to_period('W')
            df_valid['_month'] = df_valid[time_col].dt.to_period('M')
            wau = df_valid.groupby('_week')[user_id_col].nunique()
            mau = df_valid.groupby('_month')[user_id_col].nunique()
            summary['wau_avg'] = float(wau.mean()) if len(wau) > 0 else None
            summary['mau_avg'] = float(mau.mean()) if len(mau) > 0 else None

            # DAU/MAU ratio
            if summary['mau_avg'] and summary['mau_avg'] > 0:
                avg_dau = float(dau['dau'].mean()) if len(dau) > 0 else 0
                summary['dau_mau_ratio'] = round(avg_dau / summary['mau_avg'], 4)

            # Day-N retention
            first_seen = df_valid.groupby(user_id_col)['_date'].min().reset_index()
            first_seen.columns = [user_id_col, 'first_date']
            merged = df_valid.merge(first_seen, on=user_id_col)
            merged['day_offset'] = (pd.to_datetime(merged['_date']) - pd.to_datetime(merged['first_date'])).dt.days

            total_users = merged[user_id_col].nunique()
            retention_rates = {}
            for d in [1, 7, 30]:
                retained = merged[merged['day_offset'] >= d][user_id_col].nunique()
                # Users who came back on or after day d
                day_d_users = merged[merged['day_offset'] == d][user_id_col].nunique()
                retention_rates[f'day{d}'] = round(day_d_users / total_users, 4) if total_users > 0 else 0
            summary['retention_rates'] = retention_rates

            # Activity segments (by event count percentile)
            user_activity = df_valid.groupby(user_id_col).size().reset_index(name='event_count')
            q25 = user_activity['event_count'].quantile(0.25)
            q50 = user_activity['event_count'].quantile(0.50)
            q75 = user_activity['event_count'].quantile(0.75)

            def segment(cnt):
                if cnt >= q75:
                    return 'heavy'
                elif cnt >= q50:
                    return 'medium'
                elif cnt >= q25:
                    return 'light'
                else:
                    return 'dormant'

            user_activity['segment'] = user_activity['event_count'].apply(segment)
            seg_counts = user_activity['segment'].value_counts().to_dict()
            summary['activity_segments'] = {str(k): int(v) for k, v in seg_counts.items()}

        if _benchmarks:
            summary['industry_benchmarks'] = _benchmarks

        result = summary
    else:
        result = {'error': '未找到数据'}
except Exception as e:
    result = {'error': str(e)}

_cache_result('behavior_step1', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step1_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, default=str, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, default=str, indent=2))
