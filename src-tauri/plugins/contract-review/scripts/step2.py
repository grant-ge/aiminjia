# Step 2 precompute: 加载 step1 结果，生成遗漏条款检查骨架
# Depends on: _KNOWLEDGE, _ANALYSIS_DIR
import json as _json_mod
import os as _os_mod

# 从知识库加载风险模式
_risk_patterns = _KNOWLEDGE.get('risk_patterns', {}) if '_KNOWLEDGE' in dir() else {}

# 法规合规检查点（按合同类型）
COMPLIANCE_CHECKPOINTS = {
    'labor': [
        '合同期限是否明确（固定期限/无固定期限/以完成一定工作任务为期限）',
        '试用期是否符合劳动合同法规定（≤6个月，且不超过合同期1/2）',
        '工资标准是否不低于当地最低工资',
        '是否约定了法定福利（社保、公积金）',
        '竞业限制是否有经济补偿条款',
        '合同解除条件是否符合劳动合同法第36-41条',
    ],
    'purchase': [
        '标的物描述是否符合国家/行业标准',
        '知识产权条款是否涵盖定制开发场景',
        '质量保证期是否符合法规要求',
        '争议解决条款中的仲裁机构是否合法有效',
    ],
    'service': [
        '服务标准是否可量化验收',
        '知识产权归属是否明确',
        '数据安全/隐私保护条款（如涉及个人数据）',
        '分包是否需要甲方书面同意',
    ],
    'nda': [
        '保密信息范围是否过于宽泛（可能无效）',
        '保密期限是否合理（过长可能违反公序良俗）',
        '竞业限制是否有区域、时间、补偿限制',
    ],
    'sales': [
        '产品质量标准是否符合国家强制性标准',
        '知识产权担保条款（出售方保证不侵权）',
        '消费者权益保护相关条款（如面向消费者）',
        '争议解决条款中的仲裁机构是否合法有效',
    ],
    'general': [
        '主体是否具备相应资质',
        '合同形式是否符合法律要求',
        '争议解决条款是否明确有效',
    ],
}

# 加载 step1 结果
step1_path = _os_mod.path.join(_ANALYSIS_DIR, 'step1_precompute.json')
step1_data = {}
if _os_mod.path.exists(step1_path):
    with open(step1_path, 'r', encoding='utf-8') as f:
        step1_data = _json_mod.load(f)

contract_type = step1_data.get('detected_type', 'general')
compliance_items = COMPLIANCE_CHECKPOINTS.get(contract_type, COMPLIANCE_CHECKPOINTS['general'])

result = {
    'contract_type': contract_type,
    'compliance_checkpoints': compliance_items,
    'compliance_count': len(compliance_items),
    'standard_clauses': step1_data.get('standard_clauses', []),
    'instruction': '请逐项检查以上合规要点，并识别缺失的标准条款',
}

# 注入知识库：风险模式（供合规检查参考法律依据）
if _risk_patterns and 'patterns' in _risk_patterns:
    result['risk_patterns'] = _risk_patterns['patterns']

_cache_result('contract_review_step2', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step2_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, indent=2)

print(_json_mod.dumps(result, ensure_ascii=False, indent=2))
