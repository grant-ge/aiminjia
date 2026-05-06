#!/usr/bin/env python3
"""
人才结构切片分析（按部门/年龄/司龄）
用法: python3 analyze_structure.py --input <step1_normalized_scores.xlsx 或 step2_9box_mapping.xlsx> --analysis-dir <中间产物目录>
依赖: step1_precompute.json, step2_precompute.json 在 analysis-dir 中
输出: step3_precompute.json, step3_structure_analysis.xlsx
"""
import argparse
import json
import os

import pandas as pd


BOX_LABELS = {
    (3, 3): '明星人才', (3, 2): '核心骨干', (3, 1): '专业专家',
    (2, 3): '高潜新星', (2, 2): '稳定贡献者', (2, 1): '待发展者',
    (1, 3): '待激活者', (1, 2): '观察对象', (1, 1): '需改进者',
}

LEVEL_MAP = {
    'a': 3, 'b': 2, 'c': 1, 'd': 0,
    '优': 3, '良': 2, '中': 1.5, '差': 1, '不合格': 0,
    '优秀': 3, '良好': 2, '合格': 1, '待改进': 0,
    '高': 3, '中': 2, '低': 1,
    'high': 3, 'medium': 2, 'low': 1,
    'excellent': 3, 'good': 2, 'average': 1, 'poor': 0,
}


def to_numeric_fallback(series: pd.Series) -> pd.Series:
    numeric = pd.to_numeric(series, errors='coerce')
    if numeric.notna().sum() > 0:
        return numeric
    return series.astype(str).str.strip().str.lower().map(LEVEL_MAP)


def apply_3level(series: pd.Series, thresholds: dict) -> pd.Series:
    valid = series.dropna()
    if len(valid) == 0:
        return series
    q33 = thresholds.get('low_max') or float(valid.quantile(0.33))
    q66 = thresholds.get('mid_max') or float(valid.quantile(0.66))
    return series.apply(
        lambda x: 1 if pd.notna(x) and x <= q33 else (2 if pd.notna(x) and x <= q66 else (3 if pd.notna(x) else x))
    )


def dist_for_group(group_df: pd.DataFrame) -> dict:
    dist = group_df['_9box_label'].value_counts().to_dict()
    return {k: int(v) for k, v in dist.items()}


def main():
    parser = argparse.ArgumentParser(description='Step 3: 人才结构分析')
    parser.add_argument('--input', required=True)
    parser.add_argument('--analysis-dir', required=True)
    args = parser.parse_args()

    step1_path = os.path.join(args.analysis_dir, 'step1_precompute.json')
    with open(step1_path, encoding='utf-8') as f:
        step1 = json.load(f)

    detected = {item['semantic']: item['column'] for item in step1.get('field_mapping', [])}
    perf_col = step1.get('perf_col')
    pot_col  = step1.get('pot_col')
    perf_thresholds = step1.get('performance', {}).get('thresholds', {})
    pot_thresholds  = step1.get('potential',   {}).get('thresholds', {})

    path = args.input
    df = pd.read_csv(path) if path.endswith('.csv') else pd.read_excel(path)

    # 确保 _9box_label 存在（step2 的 xlsx 里应已有，否则重新计算）
    if '_9box_label' not in df.columns:
        if perf_col and pot_col and perf_col in df.columns and pot_col in df.columns:
            perf_numeric = to_numeric_fallback(df[perf_col])
            pot_numeric  = to_numeric_fallback(df[pot_col])
            df['_perf_level'] = apply_3level(perf_numeric, perf_thresholds)
            df['_pot_level']  = apply_3level(pot_numeric,  pot_thresholds)
            df['_9box_label'] = df.apply(
                lambda row: BOX_LABELS.get(
                    (int(row['_perf_level']), int(row['_pot_level'])), '未分类'
                ) if pd.notna(row.get('_perf_level')) and pd.notna(row.get('_pot_level')) else '数据缺失',
                axis=1
            )
        else:
            print(json.dumps({'error': '无法重建九宫格标签，请先运行 step2.py'}, ensure_ascii=False))
            return

    # --- 部门切片 ---
    dept_analysis = {}
    dept_col = detected.get('department')
    if dept_col and dept_col in df.columns:
        for dept in df[dept_col].dropna().unique():
            sub = df[df[dept_col] == dept]
            dist = dist_for_group(sub)
            dept_analysis[str(dept)] = {
                'total':       len(sub),
                'distribution': dist,
                'star_count':  int(dist.get('明星人才', 0)),
                'risk_count':  int(sum(dist.get(l, 0) for l in ['需改进者', '观察对象', '待激活者'])),
            }

    # --- 年龄段切片 ---
    age_analysis = {}
    age_col = detected.get('age')
    if age_col and age_col in df.columns:
        age_numeric = pd.to_numeric(df[age_col], errors='coerce')
        if age_numeric.notna().sum() > 0:
            bins   = [0, 25, 30, 35, 40, 50, 200]
            labels = ['25岁以下', '25-30岁', '30-35岁', '35-40岁', '40-50岁', '50岁以上']
            df['_age_group'] = pd.cut(age_numeric, bins=bins, labels=labels, right=False)
            for grp in labels:
                sub = df[df['_age_group'] == grp]
                if len(sub) > 0:
                    age_analysis[grp] = {'total': len(sub), 'distribution': dist_for_group(sub)}

    # --- 司龄段切片 ---
    tenure_analysis = {}
    tenure_col = detected.get('tenure')
    if tenure_col and tenure_col in df.columns:
        tenure_numeric = pd.to_numeric(df[tenure_col], errors='coerce')
        if tenure_numeric.notna().sum() > 0:
            bins   = [0, 1, 3, 5, 10, 200]
            labels = ['1年以下', '1-3年', '3-5年', '5-10年', '10年以上']
            df['_tenure_group'] = pd.cut(tenure_numeric, bins=bins, labels=labels, right=False)
            for grp in labels:
                sub = df[df['_tenure_group'] == grp]
                if len(sub) > 0:
                    tenure_analysis[grp] = {'total': len(sub), 'distribution': dist_for_group(sub)}

    result = {
        'department_analysis': dept_analysis,
        'age_analysis':        age_analysis,
        'tenure_analysis':     tenure_analysis,
    }

    out_json = os.path.join(args.analysis_dir, 'step3_precompute.json')
    with open(out_json, 'w', encoding='utf-8') as f:
        json.dump(result, f, ensure_ascii=False, default=str, indent=2)

    # 导出汇总表
    rows = []
    for dept, data in dept_analysis.items():
        for label, count in data['distribution'].items():
            rows.append({'维度': '部门', '分组': dept, '九宫格位置': label, '人数': count})
    for grp, data in age_analysis.items():
        for label, count in data['distribution'].items():
            rows.append({'维度': '年龄段', '分组': grp, '九宫格位置': label, '人数': count})
    for grp, data in tenure_analysis.items():
        for label, count in data['distribution'].items():
            rows.append({'维度': '司龄段', '分组': grp, '九宫格位置': label, '人数': count})
    if rows:
        pd.DataFrame(rows).to_excel(
            os.path.join(args.analysis_dir, 'step3_structure_analysis.xlsx'), index=False
        )

    print(json.dumps(result, ensure_ascii=False, default=str, indent=2))


if __name__ == '__main__':
    main()
