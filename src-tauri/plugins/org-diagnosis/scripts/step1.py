# Step 1: Load diagnosis frameworks and intervention library
# Executed automatically by Rust before LLM starts.
# Depends on: _KNOWLEDGE, _ANALYSIS_DIR

import json as _json_mod
import os as _os_mod

result = {}
try:
    _frameworks = _KNOWLEDGE.get('frameworks', {}) if '_KNOWLEDGE' in dir() else {}
    _interventions = _KNOWLEDGE.get('interventions', {}) if '_KNOWLEDGE' in dir() else {}

    result = {
        'frameworks': _frameworks,
        'interventions': _interventions,
        'note': '基于知识库推荐诊断框架，请结合用户组织症状做针对性选择'
    }
except Exception as e:
    result = {'error': str(e)}

with open(_os_mod.path.join(_ANALYSIS_DIR, 'step1_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, indent=2))
