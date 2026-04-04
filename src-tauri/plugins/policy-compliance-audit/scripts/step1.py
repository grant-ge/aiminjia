# Step 1 precompute: 加载劳动法规知识库，辅助逐条合规审查
# Depends on: _KNOWLEDGE, _ANALYSIS_DIR

import json as _json_mod
import os as _os_mod

result = {}
try:
    _laws = _KNOWLEDGE.get('labor_law_index', {}) if '_KNOWLEDGE' in dir() else {}
    _invalid = _KNOWLEDGE.get('invalid_clause_patterns', {}) if '_KNOWLEDGE' in dir() else {}
    _procedure = _KNOWLEDGE.get('procedure_rules', {}) if '_KNOWLEDGE' in dir() else {}

    result = {
        'labor_law_index': _laws,
        'invalid_clause_patterns': _invalid,
        'procedure_rules': _procedure,
        'note': '基于知识库进行逐条合规审查，请结合制度具体内容判断'
    }
except Exception as e:
    result = {'error': str(e)}

with open(_os_mod.path.join(_ANALYSIS_DIR, 'step1_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, indent=2))
