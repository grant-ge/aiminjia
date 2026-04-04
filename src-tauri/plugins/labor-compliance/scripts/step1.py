# Step 1: Load labor regulations and remediation measures
# Executed automatically by Rust before LLM starts.
# Depends on: _KNOWLEDGE, _ANALYSIS_DIR

import json as _json_mod
import os as _os_mod

result = {}
try:
    _regulations = _KNOWLEDGE.get('regulations', {}) if '_KNOWLEDGE' in dir() else {}
    _remediation = _KNOWLEDGE.get('remediation', {}) if '_KNOWLEDGE' in dir() else {}

    result = {
        'regulations': _regulations,
        'remediation': _remediation,
        'note': '基于知识库提供法律条文参考，请结合用户具体场景做适用性分析'
    }
except Exception as e:
    result = {'error': str(e)}

_cache_result('labor_compliance_step1', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step1_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, indent=2))
