#!/usr/bin/env python3
"""
岗位归一化
用法: python3 normalize_positions.py --input <清洗后xlsx> --analysis-dir <中间产物目录>
依赖: step1_precompute.json 在 analysis-dir 中
输出: step2_precompute.json, step2_normalization_map.xlsx
"""
import argparse
import json
import os
import re

import pandas as pd


# 岗位族关键词映射（顺序即优先级）
JOB_FAMILY_KEYWORDS = {
    '技术研发': ['研发', '开发', '工程师', '架构', 'rd', 'dev', 'engineer', 'tech', '前端', '后端', '算法', '测试', 'qa', 'sre', 'devops'],
    '产品设计': ['产品', '设计', 'pm', 'pd', 'ux', 'ui', '交互', '视觉', 'product', 'designer'],
    '市场营销': ['市场', '营销', '品牌', '公关', '推广', 'marketing', 'brand', 'pr', 'growth'],
    '销售': ['销售', '商务', '客户', 'bd', 'ae', 'am', 'kam', 'sales', 'business development', 'account'],
    '人力资源': ['人力', 'hr', 'hrbp', '招聘', '培训', '薪酬', '员工关系', 'human resource', 'talent', 'recruit'],
    '财务': ['财务', '会计', '审计', '税务', '资金', 'finance', 'accounting', 'audit', 'tax', 'treasury'],
    '法务合规': ['法务', '法律', '合规', '知识产权', 'legal', 'compliance', 'ip', 'counsel'],
    '运营': ['运营', '供应链', '物流', '仓储', '客服', 'operation', 'ops', 'supply chain', 'logistics', 'cs'],
    '行政': ['行政', '前台', '总务', '采购', 'admin', 'general affairs', 'procurement', 'facility'],
    '数据': ['数据', '分析', 'bi', 'analytics', 'data', 'analyst', 'scientist'],
    '管理层': ['总经理', 'ceo', 'cto', 'cfo', 'coo', 'vp', '副总', '总监', '部长', 'director', 'head of', 'chief'],
}

LEVEL_SUFFIXES = {
    '高级': 3, '资深': 3, 'senior': 3, 'sr': 3, 'principal': 4, 'lead': 3,
    '中级': 2, 'mid': 2, 'intermediate': 2,
    '初级': 1, '助理': 1, 'junior': 1, 'jr': 1, 'associate': 1,
    '实习': 0, 'intern': 0,
}


def infer_job_family(position: str) -> str:
    if not position or not isinstance(position, str):
        return '其他'
    pos_lower = position.lower()
    for family, keywords in JOB_FAMILY_KEYWORDS.items():
        for kw in keywords:
            if kw.lower() in pos_lower:
                return family
    return '其他'


def infer_level_hint(position: str) -> str | None:
    if not position or not isinstance(position, str):
        return None
    pos_lower = position.lower()
    for suffix, _ in sorted(LEVEL_SUFFIXES.items(), key=lambda x: -x[1]):
        if suffix.lower() in pos_lower:
            return suffix
    return None


def normalize_position(position: str) -> str:
    """简单去除层级修饰词，保留岗位核心名称"""
    if not position or not isinstance(position, str):
        return position
    result = position.strip()
    for suffix in LEVEL_SUFFIXES:
        pattern = re.compile(re.escape(suffix), re.IGNORECASE)
        result = pattern.sub('', result).strip()
    return result if result else position


def main():
    parser = argparse.ArgumentParser(description='岗位归一化')
    parser.add_argument('--input', required=True, help='清洗后数据文件（step1_cleaned_data.xlsx）')
    parser.add_argument('--analysis-dir', required=True, help='中间产物目录')
    args = parser.parse_args()

    step1_path = os.path.join(args.analysis_dir, 'step1_precompute.json')
    with open(step1_path, encoding='utf-8') as f:
        step1 = json.load(f)
    col_map = step1.get('col_map', {})

    path = args.input
    df = pd.read_csv(path) if path.endswith('.csv') else pd.read_excel(path)
    position_col = col_map.get('position')

    if not position_col or position_col not in df.columns:
        result = {
            'warning': '未识别到岗位字段，跳过归一化',
            'mapping': [],
            'family_summary': {},
        }
    else:
        # 建立去重映射
        unique_positions = df[position_col].dropna().astype(str).unique()
        mapping = []
        for pos in sorted(unique_positions):
            normalized = normalize_position(pos)
            family = infer_job_family(pos)
            level_hint = infer_level_hint(pos)
            count = int((df[position_col].astype(str) == pos).sum())
            mapping.append({
                'original': pos,
                'normalized': normalized,
                'job_family': family,
                'level_hint': level_hint,
                'count': count,
                'needs_review': family == '其他',
            })

        family_summary = {}
        for item in mapping:
            fam = item['job_family']
            family_summary[fam] = family_summary.get(fam, 0) + item['count']

        needs_review = [m for m in mapping if m['needs_review']]
        result = {
            'mapping': mapping,
            'family_summary': family_summary,
            'needs_review_count': len(needs_review),
            'needs_review': needs_review,
        }

    out_json = os.path.join(args.analysis_dir, 'step2_precompute.json')
    with open(out_json, 'w', encoding='utf-8') as f:
        json.dump(result, f, ensure_ascii=False, default=str, indent=2)

    if result.get('mapping'):
        pd.DataFrame(result['mapping']).to_excel(
            os.path.join(args.analysis_dir, 'step2_normalization_map.xlsx'), index=False
        )

    print(json.dumps(result, ensure_ascii=False, default=str, indent=2))


if __name__ == '__main__':
    main()
