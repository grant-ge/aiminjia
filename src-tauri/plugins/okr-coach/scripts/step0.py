import json as _json_mod
import os as _os_mod

result = {}
try:
    _principles = _KNOWLEDGE.get('okr_principles', {}) if '_KNOWLEDGE' in dir() else {}
    _library = _KNOWLEDGE.get('okr_library', {}) if '_KNOWLEDGE' in dir() else {}
    _metrics = _KNOWLEDGE.get('metrics_library', {}) if '_KNOWLEDGE' in dir() else {}

    result = {
        'principles': _principles,
        'okr_examples': _library,
        'metrics_library': _metrics,
        'note': '基于知识库提供OKR案例和指标参考，请结合用户具体情况个性化调整'
    }
except Exception as e:
    result = {'error': str(e)}

_cache_result('okr_coach_step0', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step0_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, indent=2))
