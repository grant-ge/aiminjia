# Step 1: Load PA maturity model and upgrade toolstack
# Executed automatically by Rust before LLM starts.
# Depends on: _KNOWLEDGE, _ANALYSIS_DIR

import json as _json_mod
import os as _os_mod

result = {}
try:
    _maturity_model = _KNOWLEDGE.get('maturity_model', {}) if '_KNOWLEDGE' in dir() else {}
    _toolstack = _KNOWLEDGE.get('toolstack', {}) if '_KNOWLEDGE' in dir() else {}

    result = {
        'maturity_model': _maturity_model,
        'toolstack': _toolstack,
        'note': '基于知识库的成熟度模型引导自评，请结合用户实际情况评估'
    }
except Exception as e:
    result = {'error': str(e)}

with open(_os_mod.path.join(_ANALYSIS_DIR, 'step1_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, indent=2))
