#!/usr/bin/env python3
"""
调薪方案计算（保守/平衡/激进三档）
用法: python3 calc_scenarios.py --input <step1_cleaned_data.xlsx> --analysis-dir <中间产物目录>
依赖: step1_precompute.json, step4_precompute.json 在 analysis-dir 中
      ${AIJIA_SKILL_DIR}/references/benchmarks.json（通过 --benchmarks-path 传入）
      ${AIJIA_SKILL_DIR}/references/rules.json（通过 --rules-path 传入）
输出: step5_precompute.json, step5_scenarios.xlsx
"""
import argparse
import json
import os

import pandas as pd


def load_json(path: str) -> dict:
    if path and os.path.exists(path):
        with open(path, encoding='utf-8') as f:
            return json.load(f)
    return {}


def calc_scenario(df: pd.DataFrame, cr_floor: float, max_pct: float) -> dict:
    targets = df[df['_cr'] < cr_floor].copy()
    total    = len(df)
    affected = len(targets)

    if affected == 0:
        return {'affected_count': 0, 'affected_pct': 0, 'annual_budget': 0, 'avg_increase_pct': 0}

    targets['_needed_increase'] = (cr_floor * targets['_midpoint'] - targets['_salary']).clip(lower=0)
    targets['_increase_pct']    = (targets['_needed_increase'] / targets['_salary']).clip(upper=max_pct)
    targets['_actual_increase'] = targets['_salary'] * targets['_increase_pct']

    monthly_budget   = float(targets['_actual_increase'].sum())
    avg_increase_pct = float(targets['_increase_pct'].mean())

    return {
        'affected_count':   affected,
        'affected_pct':     round(affected / total * 100, 1),
        'monthly_budget':   round(monthly_budget, 0),
        'annual_budget':    round(monthly_budget * 12, 0),
        'avg_increase_pct': round(avg_increase_pct * 100, 1),
    }


def main():
    parser = argparse.ArgumentParser(description='Step 5: 调薪方案')
    parser.add_argument('--input', required=True)
    parser.add_argument('--analysis-dir', required=True)
    parser.add_argument('--benchmarks-path', help='benchmarks.json 路径')
    parser.add_argument('--rules-path', help='rules.json 路径')
    parser.add_argument('--industry', help='行业（如 互联网、制造业、金融、零售），用于市场对标')
    parser.add_argument('--city-tier', help='城市级别（一线城市 / 二线城市）', default='一线城市')
    args = parser.parse_args()

    step1      = load_json(os.path.join(args.analysis_dir, 'step1_precompute.json'))
    step4      = load_json(os.path.join(args.analysis_dir, 'step4_precompute.json'))
    benchmarks = load_json(args.benchmarks_path) if args.benchmarks_path else {}
    rules      = load_json(args.rules_path)      if args.rules_path      else {}
    col_map    = step1.get('col_map', {})

    # 从 rules.json 读取调薪方案阈值，fallback 到默认值
    ft = rules.get('fairness_thresholds', {})
    cr_warning  = ft.get('compa_ratio', {}).get('warning',     0.80)
    cr_healthy  = ft.get('compa_ratio', {}).get('healthy_min', 0.85)

    SCENARIOS = {
        'conservative': {'label': '保守方案', 'cr_floor': cr_warning,         'max_increase_pct': 0.08},
        'balanced':     {'label': '平衡方案', 'cr_floor': cr_healthy,         'max_increase_pct': 0.15},
        'aggressive':   {'label': '激进方案', 'cr_floor': cr_healthy + 0.05,  'max_increase_pct': 0.25},
    }

    path = args.input
    df   = pd.read_csv(path) if path.endswith('.csv') else pd.read_excel(path)

    salary_col = col_map.get('base_salary') or col_map.get('gross')
    level_col  = col_map.get('level')

    if not salary_col or salary_col not in df.columns:
        print(json.dumps({'error': '未找到薪酬字段'}, ensure_ascii=False))
        return

    sal = pd.to_numeric(df[salary_col], errors='coerce')
    df['_salary'] = sal

    # 为每人确定薪酬中点（有职级用同级中位数，无职级用全局中位数）
    global_median = float(sal.dropna().median())
    if level_col and level_col in df.columns:
        grade_medians    = df.groupby(level_col)['_salary'].median()
        df['_midpoint']  = df[level_col].map(grade_medians).fillna(global_median)
    else:
        df['_midpoint'] = global_median

    df['_cr'] = df.apply(
        lambda row: round(row['_salary'] / row['_midpoint'], 3)
        if pd.notna(row['_salary']) and row['_midpoint'] > 0 else None,
        axis=1
    )

    valid = df.dropna(subset=['_salary', '_cr', '_midpoint'])

    # 三档方案
    scenarios_result = {}
    for key, params in SCENARIOS.items():
        scenarios_result[key] = {
            'label':              params['label'],
            'cr_floor':           params['cr_floor'],
            'max_increase_pct':   params['max_increase_pct'] * 100,
            **calc_scenario(valid, params['cr_floor'], params['max_increase_pct']),
        }

    # 优先处理人群（来自 step4 异常列表）
    priority_groups = {}
    anomaly_names = {a.get('name', '') for a in step4.get('anomaly_list', []) if a.get('name')}
    name_col = col_map.get('name')
    if anomaly_names and name_col and name_col in df.columns:
        priority_df = df[df[name_col].astype(str).isin(anomaly_names)]
        if len(priority_df) > 0:
            priority_groups['CR异常人员'] = {
                'count':         len(priority_df),
                'median_salary': round(float(priority_df['_salary'].dropna().median()), 0),
            }

    # 市场对标数据（从 benchmarks.json 读取）
    market_data = None
    if benchmarks.get('salary_percentiles') and args.industry and args.city_tier:
        industry_data = benchmarks['salary_percentiles'].get(args.industry, {})
        city_data     = industry_data.get(args.city_tier, {})
        if city_data:
            market_data = {
                'industry':   args.industry,
                'city_tier':  args.city_tier,
                'benchmarks': city_data,
                'note':       benchmarks.get('metadata', {}).get('note', ''),
            }

    result = {
        'scenarios':       scenarios_result,
        'priority_groups': priority_groups,
        'overall_cr': {
            'mean':      round(float(df['_cr'].dropna().mean()), 3),
            'median':    round(float(df['_cr'].dropna().median()), 3),
            'below_warning':  int((df['_cr'].dropna() < cr_warning).sum()),
            'below_healthy':  int((df['_cr'].dropna() < cr_healthy).sum()),
        },
        'market_data':     market_data,
        'thresholds_used': {
            'cr_warning':  cr_warning,
            'cr_healthy':  cr_healthy,
        },
    }

    out_json = os.path.join(args.analysis_dir, 'step5_precompute.json')
    with open(out_json, 'w', encoding='utf-8') as f:
        json.dump(result, f, ensure_ascii=False, default=str, indent=2)

    rows = [
        {
            '方案':          v['label'],
            'CR目标下限':     v['cr_floor'],
            '最大调幅上限(%)': v['max_increase_pct'],
            '影响人数':       v.get('affected_count', 0),
            '影响比例(%)':    v.get('affected_pct', 0),
            '年度预算增量':    v.get('annual_budget', 0),
            '平均调幅(%)':    v.get('avg_increase_pct', 0),
        }
        for v in scenarios_result.values()
    ]
    pd.DataFrame(rows).to_excel(
        os.path.join(args.analysis_dir, 'step5_scenarios.xlsx'), index=False
    )

    print(json.dumps(result, ensure_ascii=False, default=str, indent=2))


if __name__ == '__main__':
    main()
