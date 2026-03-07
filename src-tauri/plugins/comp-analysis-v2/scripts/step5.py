# Step 5: Action plan + scenarios
# Executed automatically by Rust before LLM starts.
# Depends on: _df, _ANALYSIS_DIR, _load_cached, _detect_columns,
#             _step5_scenarios, _step5_build_report_sections, _export_detail

import json as _json_mod
import pandas as _pd_mod

step1 = _load_cached('step1')
step4 = _load_cached('step4')
col_map = step1['col_map'] if step1 else _detect_columns(_df).get('detected', {})
diagnosis = step4 if step4 else {}
scenarios = _step5_scenarios(_df, col_map, diagnosis)
sections = _step5_build_report_sections(_df, col_map, diagnosis, scenarios)

# Cache precompute result
_precompute = {
    'scenarios': scenarios,
    'report_sections': sections,
}
with open(os.path.join(_ANALYSIS_DIR, 'step5_precompute.json'), 'w') as f:
    _json_mod.dump(_precompute, f, ensure_ascii=False, default=str)

# Auto-export scenarios comparison
scenario_data = scenarios.get('scenarios', {}) if isinstance(scenarios, dict) else {}
if scenario_data:
    rows = []
    for key, s in scenario_data.items():
        rows.append({
            'scenario': key,
            'description': s.get('description', ''),
            'count': s.get('count', 0),
            'annual_budget': s.get('annual_budget', 0),
            'avg_increase_pct': s.get('avg_increase_pct', 0),
            'post_cr_compliance': s.get('post_cr_compliance', ''),
        })
    _export_detail(_pd_mod.DataFrame(rows), 'step5_scenarios', '调薪方案对比')
