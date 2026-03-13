# Step 3: Channel ROI and quality analysis
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

channel_analysis = []

if 'channel' in detected:
    ch_col = detected['channel']
    channels = _df[ch_col].dropna().unique()

    for ch in channels:
        ch_df = _df[_df[ch_col] == ch]
        row = {'channel': str(ch), 'total_candidates': len(ch_df)}

        # Count at each stage
        if 'apply_date' in detected:
            row['applied'] = int(ch_df[detected['apply_date']].notna().sum())
        if 'interview_date' in detected:
            row['interviewed'] = int(ch_df[detected['interview_date']].notna().sum())
        if 'offer_date' in detected:
            row['offered'] = int(ch_df[detected['offer_date']].notna().sum())
        if 'onboard_date' in detected:
            row['onboarded'] = int(ch_df[detected['onboard_date']].notna().sum())

        # Conversion rates
        if row.get('applied', 0) > 0:
            if 'interviewed' in row:
                row['interview_rate'] = round(row['interviewed'] / row['applied'] * 100, 1)
            if 'offered' in row:
                row['offer_rate'] = round(row['offered'] / row['applied'] * 100, 1)
            if 'onboarded' in row:
                row['hire_rate'] = round(row['onboarded'] / row['applied'] * 100, 1)

        # Cost per hire
        if 'cost' in detected:
            total_cost = _pd_mod.to_numeric(ch_df[detected['cost']], errors='coerce').sum()
            row['total_cost'] = float(total_cost)
            if row.get('onboarded', 0) > 0:
                row['cost_per_hire'] = round(float(total_cost) / row['onboarded'], 0)

        # Average time to hire
        if 'apply_date' in detected and 'onboard_date' in detected:
            try:
                apply = _pd_mod.to_datetime(ch_df[detected['apply_date']], errors='coerce')
                onboard = _pd_mod.to_datetime(ch_df[detected['onboard_date']], errors='coerce')
                valid = apply.notna() & onboard.notna()
                if valid.sum() > 0:
                    row['avg_time_to_hire'] = round(float((onboard[valid] - apply[valid]).dt.days.mean()), 1)
            except Exception:
                pass

        # Probation pass rate (90-day retention proxy)
        if 'probation_result' in detected:
            prob_col = detected['probation_result']
            hired = ch_df[ch_df[detected.get('onboard_date', '')].notna()] if 'onboard_date' in detected else ch_df
            if len(hired) > 0:
                passed = hired[prob_col].astype(str).str.contains('通过|pass|转正', case=False, na=False).sum()
                row['probation_pass_rate'] = round(int(passed) / len(hired) * 100, 1)

        channel_analysis.append(row)

_precompute = {
    'channel_analysis': channel_analysis,
    'total_channels': len(channel_analysis),
}

with open(os.path.join(_ANALYSIS_DIR, 'step3_precompute.json'), 'w') as f:
    _json_mod.dump(_precompute, f, ensure_ascii=False, default=str)
print(_json_mod.dumps(_precompute, ensure_ascii=False, default=str, indent=2))

# Auto-export
if channel_analysis:
    _export_detail(_pd_mod.DataFrame(channel_analysis), 'step3_channel_analysis', '渠道分析明细')
