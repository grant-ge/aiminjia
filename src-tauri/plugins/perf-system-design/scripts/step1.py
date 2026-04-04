# Step 1: Load performance frameworks and industry templates
# Executed automatically by Rust before LLM starts.
# Depends on: _KNOWLEDGE, _ANALYSIS_DIR

import json as _json_mod
import os as _os_mod

result = {}
try:
    _frameworks = _KNOWLEDGE.get('frameworks', {}) if '_KNOWLEDGE' in dir() else {}
    _templates = _KNOWLEDGE.get('templates', {}) if '_KNOWLEDGE' in dir() else {}

    result = {
        'frameworks': _frameworks,
        'templates': _templates,
        'note': '基于知识库推荐绩效模式，请结合用户企业信息做个性化适配'
    }
except Exception as e:
    result = {'error': str(e)}

_cache_result('perf_system_step1', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step1_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, indent=2))
