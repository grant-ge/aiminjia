# Step 1 precompute: 根据合同类型生成标准条款检查清单
# Depends on: _KNOWLEDGE, _ANALYSIS_DIR, _text（合同文本，从 load_file 注入）
# step0 的 save_analysis_note 结果在 _ANALYSIS_DIR/step0_notes.json 中可能有，但不能依赖

import json as _json_mod
import os as _os_mod
import re as _re_mod

# 从知识库加载条款库和风险模式
_clause_library = _KNOWLEDGE.get('clause_library', {}) if '_KNOWLEDGE' in dir() else {}
_risk_patterns = _KNOWLEDGE.get('risk_patterns', {}) if '_KNOWLEDGE' in dir() else {}

# 各类合同的标准条款清单
CONTRACT_STANDARD_CLAUSES = {
    'purchase': ['合同主体', '标的物描述', '数量规格', '价款支付', '交付方式', '质量验收', '违约责任', '知识产权', '保密条款', '争议解决', '合同期限'],
    'service': ['服务范围', '服务标准', '付款条款', '验收标准', '保密条款', '知识产权归属', '违约责任', '合同变更', '争议解决', '合同期限'],
    'labor': ['工作岗位', '薪酬待遇', '工作时间', '社会保险', '保密义务', '竞业限制', '合同期限', '试用期', '解除条件', '违约责任'],
    'nda': ['保密信息定义', '保密义务', '保密期限', '例外情形', '违约责任', '信息归还销毁', '适用法律'],
    'sales': ['产品描述', '价格条款', '付款方式', '交货条款', '风险转移', '质量保证', '违约责任', '知识产权', '保密条款', '争议解决'],
    'general': ['合同主体', '合同标的', '价款条款', '权利义务', '违约责任', '保密条款', '知识产权', '争议解决', '合同期限', '变更解除'],
}

# 尝试从合同文本判断类型（简单关键词匹配，LLM step0 已做精确识别）
def _guess_contract_type(text):
    text_lower = text.lower() if text else ''
    if any(k in text_lower for k in ['劳动合同', '劳动关系', '用工', 'employment']):
        return 'labor'
    if any(k in text_lower for k in ['保密', '保密协议', 'nda', 'confidential']):
        return 'nda'
    if any(k in text_lower for k in ['采购', '购买', '供货', 'purchase', 'procurement']):
        return 'purchase'
    if any(k in text_lower for k in ['销售', '买卖', 'sales', 'sell']):
        return 'sales'
    if any(k in text_lower for k in ['服务', '委托', 'service', 'outsource']):
        return 'service'
    return 'general'

contract_text = _text if '_text' in dir() else ''
_file_is_table = False
if not contract_text and '_dfs' in dir() and _dfs:
    # Excel/CSV format: concatenate cell values for keyword detection
    _file_is_table = True
    _parts = []
    for _sheet_name, _df in _dfs.items():
        _parts.append(_sheet_name)
        _parts.extend(str(v) for v in _df.values.flatten() if str(v) != 'nan')
    contract_text = ' '.join(_parts[:500])  # cap to avoid oversized string
contract_type = _guess_contract_type(contract_text)
standard_clauses = CONTRACT_STANDARD_CLAUSES.get(contract_type, CONTRACT_STANDARD_CLAUSES['general'])

result = {
    'detected_type': contract_type,
    'standard_clauses': standard_clauses,
    'clause_count': len(standard_clauses),
    'note': '此清单仅为参考结构，请根据实际合同内容判断条款是否存在及风险程度',
}
if _file_is_table:
    result['warning'] = '合同文件为表格格式（Excel/CSV），类型检测基于单元格内容关键词，准确度可能较低'

# 注入知识库：条款库（含必备要素和常见缺陷）
_type_clauses = _clause_library.get(contract_type, {})
if _type_clauses:
    result['clause_library'] = _type_clauses

# 注入知识库：风险模式（常见合同风险及法律依据）
if _risk_patterns and 'patterns' in _risk_patterns:
    result['risk_patterns'] = _risk_patterns['patterns']

with open(_os_mod.path.join(_ANALYSIS_DIR, 'step1_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, indent=2)

print(_json_mod.dumps(result, ensure_ascii=False, indent=2))
