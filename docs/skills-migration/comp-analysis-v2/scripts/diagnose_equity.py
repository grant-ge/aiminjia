#!/usr/bin/env python3
"""
薪酬公平性诊断（CR 值、区间渗透率、倒挂、离群值）
用法: python3 diagnose_equity.py --input <step1_cleaned_data.xlsx> --analysis-dir <中间产物目录>
依赖: step1_precompute.json, step3_precompute.json 在 analysis-dir 中
      ${AIJIA_SKILL_DIR}/references/rules.json（通过 --rules-path 传入）
输出: step4_precompute.json, step4_anomaly_detail.xlsx
"""
import argparse
import json
import os

import pandas as pd
import numpy as np


def load_json(path: str) -> dict:
    if path and os.path.exists(path):
        with open(path, encoding='utf-8') as f:
            return json.load(f)
    return {}


def compute_cr(salary: float, midpoint: float) -> float | None:
    if midpoint and midpoint > 0:
        return round(salary / midpoint, 3)
    return None


def penetration_rate(salary: float, band_min: float, band_max: float) -> float | None:
    if band_max > band_min:
        return round((salary - band_min) / (band_max - band_min), 3)
    return None


def main():
    parser = argparse.ArgumentParser(description='薪酬公平性诊断')
    parser.add_argument('--input', required=True)
    parser.add_argument('--analysis-dir', required=True)
    parser.add_argument('--rules-path', help='rules.json 路径（${AIJIA_SKILL_DIR}/references/rules.json）')
    parser.add_argument('--location', help='主要工作城市，用于最低工资合规检查（如 北京、上海）')
    args = parser.parse_args()

    step1 = load_json(os.path.join(args.analysis_dir, 'step1_precompute.json'))
    step3 = load_json(os.path.join(args.analysis_dir, 'step3_precompute.json'))
    rules = load_json(args.rules_path) if args.rules_path else {}
    col_map = step1.get('col_map', {})

    # 从 rules.json 读取阈值，fallback 到保守默认值
    ft = rules.get('fairness_thresholds', {})
    cr_healthy_min  = ft.get('compa_ratio', {}).get('healthy_min', 0.85)
    cr_healthy_max  = ft.get('compa_ratio', {}).get('healthy_max', 1.15)
    cr_warning      = ft.get('compa_ratio', {}).get('warning', 0.80)
    cr_critical     = ft.get('compa_ratio', {}).get('critical', 0.75)
    pen_underpaid   = ft.get('range_penetration', {}).get('underpaid', 0.25)
    pen_overpaid    = ft.get('range_penetration', {}).get('overpaid', 0.85)
    cv_low          = ft.get('cv_coefficient', {}).get('low', 0.15)
    cv_moderate     = ft.get('cv_coefficient', {}).get('moderate', 0.25)
    cv_high         = ft.get('cv_coefficient', {}).get('high', 0.35)

    path = args.input
    df = pd.read_csv(path) if path.endswith('.csv') else pd.read_excel(path)

    salary_col   = col_map.get('base_salary') or col_map.get('gross')
    level_col    = col_map.get('level')
    position_col = col_map.get('position')
    hire_col     = col_map.get('hire_date')
    name_col     = col_map.get('name')
    location_col = col_map.get('location')

    if not salary_col or salary_col not in df.columns:
        result = {'error': '未找到薪酬字段，无法诊断'}
        print(json.dumps(result, ensure_ascii=False))
        return

    sal = pd.to_numeric(df[salary_col], errors='coerce')
    df['_salary'] = sal
    valid_sal = sal.dropna()

    # 整体分布
    cv_val = float(valid_sal.std() / valid_sal.mean()) if valid_sal.mean() > 0 else None
    cv_level = None
    if cv_val is not None:
        if cv_val <= cv_low:       cv_level = '低（薪酬分布集中）'
        elif cv_val <= cv_moderate: cv_level = '中等'
        elif cv_val <= cv_high:    cv_level = '较高（薪酬离散度大）'
        else:                       cv_level = '高（薪酬极度分散，需关注）'

    overall = {
        'count':    int(len(valid_sal)),
        'mean':     round(float(valid_sal.mean()), 0),
        'median':   round(float(valid_sal.median()), 0),
        'p25':      round(float(valid_sal.quantile(0.25)), 0),
        'p75':      round(float(valid_sal.quantile(0.75)), 0),
        'std':      round(float(valid_sal.std()), 0),
        'cv':       round(cv_val, 3) if cv_val is not None else None,
        'cv_level': cv_level,
    }

    anomaly_list = []

    # --- CR 值 + 区间渗透率（按岗位+职级分组）---
    group_cols = [c for c in [level_col, position_col] if c and c in df.columns]
    cr_summary = {}

    if group_cols:
        for group_key, group_df in df.groupby(group_cols):
            group_sal = group_df['_salary'].dropna()
            if len(group_sal) < 2:
                continue
            group_median = float(group_sal.median())
            group_min    = float(group_sal.min())
            group_max    = float(group_sal.max())

            key_str = str(group_key) if not isinstance(group_key, tuple) else '_'.join(str(k) for k in group_key)
            cr_vals = []
            cr_method = 'group_median'

            for idx, row in group_df.iterrows():
                s = row.get('_salary')
                if pd.isna(s):
                    continue

                # 优先使用薪酬带中点（step3 grades 有 salary_min/salary_max 时），否则用同组中位数
                grade_info = next(
                    (g for g in step3.get('grades', []) if str(g.get('grade', '')) == str(row.get(level_col, ''))),
                    {}
                ) if level_col and step3.get('grades') else {}

                if grade_info.get('salary_min') and grade_info.get('salary_max'):
                    midpoint  = (grade_info['salary_min'] + grade_info['salary_max']) / 2
                    band_min  = grade_info['salary_min']
                    band_max  = grade_info['salary_max']
                    cr_method = 'band_midpoint'
                else:
                    midpoint  = group_median
                    band_min  = group_min
                    band_max  = group_max
                    cr_method = 'group_median'

                cr  = compute_cr(float(s), midpoint)
                pen = penetration_rate(float(s), band_min, band_max)
                cr_vals.append(cr)

                rec = {
                    'group':       key_str,
                    'cr':          cr,
                    'penetration': pen,
                    'cr_method':   cr_method,
                    'salary':      float(s),
                }
                if name_col and name_col in df.columns:
                    rec['name'] = str(row.get(name_col, ''))

                # CR 异常判断（使用 rules.json 阈值）
                if cr is not None:
                    if cr <= cr_critical:
                        rec['anomaly_type'] = f'CR严重偏低（≤{cr_critical}）'
                        anomaly_list.append(rec)
                    elif cr <= cr_warning:
                        rec['anomaly_type'] = f'CR偏低警告（≤{cr_warning}）'
                        anomaly_list.append(rec)
                    elif cr > cr_healthy_max:
                        rec['anomaly_type'] = f'CR偏高（>{cr_healthy_max}）'
                        anomaly_list.append(rec)

                # 区间渗透率判断（使用 rules.json 阈值）
                if pen is not None and cr is None:  # 避免和 CR 判断重复
                    if pen < pen_underpaid:
                        rec['anomaly_type'] = f'区间渗透率偏低（<{pen_underpaid}，处于区间下段）'
                        anomaly_list.append(rec)
                    elif pen > pen_overpaid:
                        rec['anomaly_type'] = f'区间渗透率偏高（>{pen_overpaid}，处于区间上段或超出）'
                        anomaly_list.append(rec)
                    elif pen < 0 or pen > 1:
                        rec['anomaly_type'] = '薪酬超出区间'
                        anomaly_list.append(rec)

            cr_summary[key_str] = {
                'count':     len(group_sal),
                'median':    round(group_median, 0),
                'cr_mean':   round(float(np.mean([c for c in cr_vals if c is not None])), 3) if cr_vals else None,
                'cr_method': cr_method,
            }

    # --- 倒挂检测 ---
    inversion_list = []

    # 新老员工倒挂
    if hire_col and hire_col in df.columns and group_cols:
        hire_dates = pd.to_datetime(df[hire_col], errors='coerce')
        df['_hire_date'] = hire_dates
        for group_key, group_df in df.groupby(group_cols):
            gd = group_df.dropna(subset=['_hire_date', '_salary'])
            if len(gd) < 3:
                continue
            gd = gd.sort_values('_hire_date')
            new_cutoff = gd['_hire_date'].quantile(0.3)
            new_sal = float(gd[gd['_hire_date'] > new_cutoff]['_salary'].median())
            old_sal = float(gd[gd['_hire_date'] <= new_cutoff]['_salary'].median())
            if new_sal > old_sal * 1.1:
                key_str = str(group_key) if not isinstance(group_key, tuple) else '_'.join(str(k) for k in group_key)
                inversion_list.append({
                    'type':        '新老员工薪酬倒挂',
                    'group':       key_str,
                    'new_median':  round(new_sal, 0),
                    'old_median':  round(old_sal, 0),
                    'gap_pct':     round((new_sal - old_sal) / old_sal * 100, 1),
                })

    # 跨级别倒挂
    if level_col and level_col in df.columns:
        grade_medians = df.groupby(level_col)['_salary'].median().sort_values()
        prev_grade, prev_median = None, None
        for grade, median in grade_medians.items():
            if prev_grade is not None and median < prev_median:
                inversion_list.append({
                    'type':          '级别薪酬倒挂',
                    'grade_lower':   str(prev_grade),
                    'grade_higher':  str(grade),
                    'lower_median':  round(float(prev_median), 0),
                    'higher_median': round(float(median), 0),
                })
            prev_grade, prev_median = grade, median

    # --- 最低工资合规检查 ---
    compliance_warnings = []
    min_wage_map = rules.get('minimum_wage_2025', {})
    if min_wage_map and salary_col:
        # 确定城市：优先 --location 参数，其次 location 字段
        cities_to_check = []
        if args.location:
            cities_to_check = [args.location]
        elif location_col and location_col in df.columns:
            cities_to_check = df[location_col].dropna().unique().tolist()

        for city in cities_to_check:
            min_wage = min_wage_map.get(str(city))
            if min_wage:
                below_min = df[df['_salary'] < min_wage]
                if len(below_min) > 0:
                    compliance_warnings.append({
                        'type':        '疑似低于最低工资',
                        'city':        city,
                        'min_wage':    min_wage,
                        'count':       len(below_min),
                        'note':        '仅供合规复核参考，以当地最新法规和企业制度为准',
                    })

    result = {
        'overall':              overall,
        'cr_summary':           cr_summary,
        'anomaly_list':         anomaly_list,
        'anomaly_count':        len(anomaly_list),
        'inversion_list':       inversion_list,
        'inversion_count':      len(inversion_list),
        'compliance_warnings':  compliance_warnings,
        'thresholds_used': {
            'cr_critical':    cr_critical,
            'cr_warning':     cr_warning,
            'cr_healthy_min': cr_healthy_min,
            'cr_healthy_max': cr_healthy_max,
            'pen_underpaid':  pen_underpaid,
            'pen_overpaid':   pen_overpaid,
        },
    }

    out_json = os.path.join(args.analysis_dir, 'step4_precompute.json')
    with open(out_json, 'w', encoding='utf-8') as f:
        json.dump(result, f, ensure_ascii=False, default=str, indent=2)

    all_issues = anomaly_list + inversion_list + compliance_warnings
    if all_issues:
        pd.DataFrame(all_issues).to_excel(
            os.path.join(args.analysis_dir, 'step4_anomaly_detail.xlsx'), index=False
        )

    print(json.dumps(result, ensure_ascii=False, default=str, indent=2))


if __name__ == '__main__':
    main()
