# Step 3: Grade/level inference
# Executed automatically by Rust before LLM starts.
# Depends on: _df, _ANALYSIS_DIR, _step3_grading, _export_detail

import json as _json_mod
import pandas as _pd_mod

result = _step3_grading(_df)

# Cache precompute result
_precompute = {
    'grading': result,
}
with open(os.path.join(_ANALYSIS_DIR, 'step3_precompute.json'), 'w') as f:
    _json_mod.dump(_precompute, f, ensure_ascii=False, default=str)

# Auto-export grade anomalies detail
anomalies = result.get('anomalies', [])
if anomalies:
    _export_detail(_pd_mod.DataFrame(anomalies), 'step3_grade_anomalies', '职级异常明细')
