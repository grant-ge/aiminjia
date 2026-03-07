# Step 2: Job normalization
# Executed automatically by Rust before LLM starts.
# Depends on: _df, _ANALYSIS_DIR, _step2_normalize, _export_detail

import json as _json_mod
import pandas as _pd_mod

result = _step2_normalize(_df)

# Cache precompute result
_precompute = {
    'normalization': result,
}
with open(os.path.join(_ANALYSIS_DIR, 'step2_precompute.json'), 'w') as f:
    _json_mod.dump(_precompute, f, ensure_ascii=False, default=str)

# Auto-export normalization mapping
mapping = result.get('normalization', {}).get('mapping', [])
if mapping:
    mapping_df = _pd_mod.DataFrame(mapping)
    _export_detail(mapping_df, 'step2_normalization_map', '岗位归一化映射')
