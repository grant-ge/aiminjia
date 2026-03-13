# Step 2: Funnel conversion rate calculation
# Executed automatically by Rust before LLM starts.
# Depends on: _df, _ANALYSIS_DIR, _export_detail

import json as _json_mod
import pandas as _pd_mod

# Load step1 cache for field mapping
try:
    step1_cache_path = os.path.join(_ANALYSIS_DIR, 'step1_precompute.json')
    with open(step1_cache_path, 'r') as f:
        step1_cache = _json_mod.load(f)
except (FileNotFoundError, _json_mod.JSONDecodeError):
    step1_cache = {'field_mapping': []}

detected = {item['semantic']: item['column'] for item in step1_cache.get('field_mapping', [])}

# --- Define funnel stages ---
stage_date_fields = ['apply_date', 'resume_screen_date', 'interview_date', 'interview2_date', 'offer_date', 'onboard_date']
stage_names = ['投递', '简历筛选', '初面', '复面', 'Offer', '入职']

# Count candidates at each stage
funnel = []
for i, (field, name) in enumerate(zip(stage_date_fields, stage_names)):
    if field in detected:
        col = detected[field]
        count = int(_df[col].notna().sum())
    else:
        # Try to detect from status column
        count = 0
    funnel.append({'stage': name, 'field': field, 'count': count})

# Calculate conversion rates
for i in range(1, len(funnel)):
    prev_count = funnel[i-1]['count']
    curr_count = funnel[i]['count']
    if prev_count > 0:
        funnel[i]['conversion_rate'] = round(curr_count / prev_count * 100, 1)
    else:
        funnel[i]['conversion_rate'] = 0
funnel[0]['conversion_rate'] = 100.0

# --- Calculate average time between stages ---
time_metrics = []
for i in range(1, len(stage_date_fields)):
    prev_field = stage_date_fields[i-1]
    curr_field = stage_date_fields[i]
    if prev_field in detected and curr_field in detected:
        prev_col = detected[prev_field]
        curr_col = detected[curr_field]
        try:
            prev_dates = _pd_mod.to_datetime(_df[prev_col], errors='coerce')
            curr_dates = _pd_mod.to_datetime(_df[curr_col], errors='coerce')
            valid = prev_dates.notna() & curr_dates.notna()
            if valid.sum() > 0:
                diff = (curr_dates[valid] - prev_dates[valid]).dt.days
                avg_days = round(float(diff.mean()), 1)
                time_metrics.append({
                    'from': stage_names[i-1],
                    'to': stage_names[i],
                    'avg_days': avg_days,
                    'sample_size': int(valid.sum()),
                })
        except Exception:
            pass

# --- Channel distribution ---
channel_dist = []
if 'channel' in detected:
    ch_col = detected['channel']
    ch_counts = _df[ch_col].value_counts()
    total = len(_df)
    for ch, cnt in ch_counts.items():
        channel_dist.append({
            'channel': str(ch),
            'count': int(cnt),
            'percentage': round(cnt / total * 100, 1) if total > 0 else 0,
        })

# --- Overall time to hire ---
overall_tth = None
if 'apply_date' in detected and 'onboard_date' in detected:
    try:
        apply = _pd_mod.to_datetime(_df[detected['apply_date']], errors='coerce')
        onboard = _pd_mod.to_datetime(_df[detected['onboard_date']], errors='coerce')
        valid = apply.notna() & onboard.notna()
        if valid.sum() > 0:
            overall_tth = round(float((onboard[valid] - apply[valid]).dt.days.mean()), 1)
    except Exception:
        pass

_precompute = {
    'funnel': funnel,
    'time_metrics': time_metrics,
    'channel_distribution': channel_dist,
    'overall_time_to_hire_days': overall_tth,
}

with open(os.path.join(_ANALYSIS_DIR, 'step2_precompute.json'), 'w') as f:
    _json_mod.dump(_precompute, f, ensure_ascii=False, default=str)
print(_json_mod.dumps(_precompute, ensure_ascii=False, default=str, indent=2))

# Auto-export funnel data
funnel_df = _pd_mod.DataFrame(funnel)
_export_detail(funnel_df, 'step2_funnel_data', '漏斗转化率数据')
