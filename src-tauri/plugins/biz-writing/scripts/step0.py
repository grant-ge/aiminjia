import json as _json_mod
import os as _os_mod

result = {}
try:
    _doc_types = _KNOWLEDGE.get('doc_types', {}) if '_KNOWLEDGE' in dir() else {}
    _templates = _KNOWLEDGE.get('templates', {}) if '_KNOWLEDGE' in dir() else {}
    _rules = _KNOWLEDGE.get('writing_rules', {}) if '_KNOWLEDGE' in dir() else {}

    result = {
        'doc_types': _doc_types,
        'templates': _templates,
        'writing_rules': _rules,
        'note': '基于知识库提供文档模板和写作规范'
    }
except Exception as e:
    result = {'error': str(e)}

with open(_os_mod.path.join(_ANALYSIS_DIR, 'step0_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, indent=2))
