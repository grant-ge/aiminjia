# Step 4: Compensation fairness diagnosis
# Executed automatically by Rust before LLM starts.
# Depends on: _df, _ANALYSIS_DIR, _load_cached, _detect_columns, _step4_diagnose, _export_detail

import json as _json_mod
import pandas as _pd_mod

step1 = _load_cached('step1')
col_map = step1.get('col_map') if step1 else _detect_columns(_df)
# col_map may be full result dict or just detected sub-dict
if 'detected' not in col_map and isinstance(col_map, dict):
    col_map = {'detected': col_map}
result = _step4_diagnose(_df, col_map)

# Cache precompute result
_precompute = {
    'diagnosis': result,
}
with open(os.path.join(_ANALYSIS_DIR, 'step4_precompute.json'), 'w') as f:
    _json_mod.dump(_precompute, f, ensure_ascii=False, default=str)

# Auto-export anomaly list as detail
anomaly_list = result.get('anomaly_list', [])
if anomaly_list:
    _export_detail(_pd_mod.DataFrame(anomaly_list), 'step4_anomaly_detail', '诊断异常明细')
