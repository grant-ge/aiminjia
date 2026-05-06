#!/usr/bin/env python3
"""
职级框架推断
用法: python3 infer_grades.py --input <step1_cleaned_data.xlsx> --analysis-dir <中间产物目录>
依赖: step1_precompute.json, step2_precompute.json 在 analysis-dir 中
输出: step3_precompute.json, step3_grade_anomalies.xlsx
"""
import argparse
import json
import os

import pandas as pd
import numpy as np


def main():
    parser = argparse.ArgumentParser(description='Step 3: 职级推断')
    parser.add_argument('--input', required=True)
    parser.add_argument('--analysis-dir', required=True)
    args = parser.parse_args()

    step1_path = os.path.join(args.analysis_dir, 'step1_precompute.json')
    with open(step1_path, encoding='utf-8') as f:
        step1 = json.load(f)
    col_map = step1.get('col_map', {})

    step2_path = os.path.join(args.analysis_dir, 'step2_precompute.json')
    step2 = {}
    if os.path.exists(step2_path):
        with open(step2_path, encoding='utf-8') as f:
            step2 = json.load(f)

    path = args.input
    df = pd.read_csv(path) if path.endswith('.csv') else pd.read_excel(path)

    level_col   = col_map.get('level')
    salary_col  = col_map.get('base_salary') or col_map.get('gross')
    position_col = col_map.get('position')

    grades = []        # 推断出的职级框架
    anomalies = []     # 异常记录

    # --- 方式 A：已有职级字段，直接统计 ---
    if level_col and level_col in df.columns:
        salary_numeric = pd.to_numeric(df[salary_col], errors='coerce') if salary_col else None

        for grade_val in sorted(df[level_col].dropna().unique(), key=str):
            grade_df = df[df[level_col] == grade_val]
            entry = {
                'grade':  str(grade_val),
                'source': 'existing_field',
                'count':  int(len(grade_df)),
            }
            if salary_numeric is not None:
                sal = salary_numeric[grade_df.index].dropna()
                if len(sal) > 0:
                    entry.update({
                        'salary_min':    round(float(sal.min()), 0),
                        'salary_max':    round(float(sal.max()), 0),
                        'salary_median': round(float(sal.median()), 0),
                        'salary_mean':   round(float(sal.mean()), 0),
                    })
            grades.append(entry)

        # 检测跨级薪酬重叠
        if salary_numeric is not None and len(grades) >= 2:
            for i in range(len(grades) - 1):
                g_lo, g_hi = grades[i], grades[i + 1]
                if 'salary_max' in g_lo and 'salary_min' in g_hi:
                    if g_lo['salary_max'] > g_hi['salary_min']:
                        anomalies.append({
                            'type':    '跨级薪酬重叠',
                            'grade_a': g_lo['grade'],
                            'grade_b': g_hi['grade'],
                            'overlap': f"{g_lo['grade']} 上限({g_lo['salary_max']}) > {g_hi['grade']} 下限({g_hi['salary_min']})",
                        })

    # --- 方式 B：无职级字段，按薪酬分位推断 ---
    elif salary_col and salary_col in df.columns:
        salary_numeric = pd.to_numeric(df[salary_col], errors='coerce').dropna()
        if len(salary_numeric) >= 10:
            percentiles = [0, 20, 40, 60, 80, 100]
            cuts = salary_numeric.quantile([p / 100 for p in percentiles])
            grade_names = ['G1', 'G2', 'G3', 'G4', 'G5']
            for i, name in enumerate(grade_names):
                lo = float(cuts.iloc[i])
                hi = float(cuts.iloc[i + 1])
                mask = (salary_numeric >= lo) & (salary_numeric <= hi)
                grades.append({
                    'grade':       name,
                    'source':      'salary_percentile_inferred',
                    'count':       int(mask.sum()),
                    'salary_min':  round(lo, 0),
                    'salary_max':  round(hi, 0),
                    'note':        '按薪酬五分位推断，仅供参考，需用户确认',
                })

    # --- 识别无法定级人员 ---
    if level_col and level_col in df.columns:
        null_level = df[df[level_col].isna()]
        for _, row in null_level.iterrows():
            rec = {'type': '缺失职级'}
            if col_map.get('name') in df.columns:
                rec['name'] = str(row.get(col_map['name'], ''))
            if salary_col and salary_col in df.columns:
                rec['salary'] = row.get(salary_col)
            anomalies.append(rec)

    result = {
        'grades':           grades,
        'grade_count':      len(grades),
        'inference_method': 'existing_field' if level_col else 'salary_percentile',
        'anomalies':        anomalies,
        'anomaly_count':    len(anomalies),
    }

    out_json = os.path.join(args.analysis_dir, 'step3_precompute.json')
    with open(out_json, 'w', encoding='utf-8') as f:
        json.dump(result, f, ensure_ascii=False, default=str, indent=2)

    if anomalies:
        pd.DataFrame(anomalies).to_excel(
            os.path.join(args.analysis_dir, 'step3_grade_anomalies.xlsx'), index=False
        )

    print(json.dumps(result, ensure_ascii=False, default=str, indent=2))


if __name__ == '__main__':
    main()
