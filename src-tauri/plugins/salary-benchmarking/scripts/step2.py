# Step 2: Market comparison analysis
# Depends on: _df, _ANALYSIS_DIR, _export_detail

import json as _json_mod
import pandas as _pd_mod

try:
    step1_path = os.path.join(_ANALYSIS_DIR, 'step1_precompute.json')
    with open(step1_path, 'r') as f:
        step1 = _json_mod.load(f)
except (FileNotFoundError, _json_mod.JSONDecodeError):
    step1 = {'field_mapping': []}

detected = {item['semantic']: item['column'] for item in step1.get('field_mapping', [])}
salary_col = step1.get('salary_column')
has_market = step1.get('has_market_data', False)

market_comparison = []

if has_market and salary_col:
    # Compare internal vs market data
    salary_data = _pd_mod.to_numeric(_df[salary_col], errors='coerce')

    group_col = detected.get('position') or detected.get('level') or detected.get('department')
    if group_col:
        for grp in _df[group_col].dropna().unique():
            grp_mask = _df[group_col] == grp
            grp_salary = salary_data[grp_mask].dropna()
            if len(grp_salary) < 2:
                continue

            row = {
                'group': str(grp),
                'count': int(len(grp_salary)),
                'internal_p50': round(float(grp_salary.median()), 0),
            }

            # Get market data
            for mkt_key, mkt_label in [('market_p25', 'P25'), ('market_p50', 'P50'), ('market_p75', 'P75')]:
                if mkt_key in detected:
                    mkt_val = _pd_mod.to_numeric(_df.loc[grp_mask, detected[mkt_key]], errors='coerce').median()
                    if _pd_mod.notna(mkt_val):
                        row[f'market_{mkt_label}'] = round(float(mkt_val), 0)

            # Calculate competitiveness ratio
            if 'market_P50' in row and row['market_P50'] > 0:
                row['competitiveness_ratio'] = round(row['internal_p50'] / row['market_P50'] * 100, 1)
                row['position'] = '领先' if row['competitiveness_ratio'] > 105 else ('持平' if row['competitiveness_ratio'] > 95 else '滞后')

            market_comparison.append(row)
else:
    # No market data - internal relative positioning only
    if salary_col:
        salary_data = _pd_mod.to_numeric(_df[salary_col], errors='coerce')
        _median = salary_data.median()
        overall_median = float(_median) if _pd_mod.notna(_median) else 0
        group_col = detected.get('position') or detected.get('level') or detected.get('department')
        if group_col:
            for grp in _df[group_col].dropna().unique():
                grp_salary = salary_data[_df[group_col] == grp].dropna()
                if len(grp_salary) < 2:
                    continue
                grp_median = float(grp_salary.median())
                market_comparison.append({
                    'group': str(grp),
                    'count': int(len(grp_salary)),
                    'internal_p50': round(grp_median, 0),
                    'vs_overall': round(grp_median / overall_median * 100, 1) if overall_median > 0 else 0,
                    'note': '无市场数据，仅展示内部相对定位',
                })

_precompute = {
    'has_market_data': has_market,
    'market_comparison': market_comparison,
}

with open(os.path.join(_ANALYSIS_DIR, 'step2_precompute.json'), 'w') as f:
    _json_mod.dump(_precompute, f, ensure_ascii=False, default=str)
print(_json_mod.dumps(_precompute, ensure_ascii=False, default=str, indent=2))

if market_comparison:
    _export_detail(_pd_mod.DataFrame(market_comparison), 'step2_market_comparison', '市场对位分析')
