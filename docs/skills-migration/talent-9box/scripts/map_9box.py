#!/usr/bin/env python3
"""
九宫格定位与分布
用法: python3 map_9box.py --input <step1_normalized_scores.xlsx> --analysis-dir <中间产物目录>
依赖: step1_precompute.json 在 analysis-dir 中
      ${AIJIA_SKILL_DIR}/references/benchmarks.json（通过 --benchmarks-path 传入）
输出: step2_precompute.json, step2_9box_mapping.xlsx
"""
import argparse
import json
import os

import pandas as pd


BOX_LABELS = {
    (3, 3): ('明星人才',  'Star'),
    (3, 2): ('核心骨干',  'Core Player'),
    (3, 1): ('专业专家',  'Expert'),
    (2, 3): ('高潜新星',  'Rising Star'),
    (2, 2): ('稳定贡献者', 'Steady Contributor'),
    (2, 1): ('待发展者',  'Needs Development'),
    (1, 3): ('待激活者',  'Underperformer w/ Potential'),
    (1, 2): ('观察对象',  'Watch List'),
    (1, 1): ('需改进者',  'Underperformer'),
}

# benchmarks.json 中格位 key → BOX_LABELS key 映射
BENCHMARK_KEY_MAP = {
    '高绩效-高潜力': '明星人才',
    '高绩效-中潜力': '核心骨干',
    '高绩效-低潜力': '专业专家',
    '中绩效-高潜力': '高潜新星',
    '中绩效-中潜力': '稳定贡献者',
    '中绩效-低潜力': '待发展者',
    '低绩效-高潜力': '待激活者',
    '低绩效-中潜力': '观察对象',
    '低绩效-低潜力': '需改进者',
}

LEVEL_MAP = {
    'a': 3, 'b': 2, 'c': 1, 'd': 0,
    '优': 3, '良': 2, '中': 1.5, '差': 1, '不合格': 0,
    '优秀': 3, '良好': 2, '合格': 1, '待改进': 0,
    '高': 3, '中': 2, '低': 1,
    'high': 3, 'medium': 2, 'low': 1,
    'excellent': 3, 'good': 2, 'average': 1, 'poor': 0,
}


def load_json(path: str) -> dict:
    if path and os.path.exists(path):
        with open(path, encoding='utf-8') as f:
            return json.load(f)
    return {}


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


def main():
    parser = argparse.ArgumentParser(description='Step 2: 九宫格定位')
    parser.add_argument('--input', required=True, help='step1_normalized_scores.xlsx')
    parser.add_argument('--analysis-dir', required=True)
    parser.add_argument('--benchmarks-path', help='benchmarks.json 路径')
    args = parser.parse_args()

    step1 = load_json(os.path.join(args.analysis_dir, 'step1_precompute.json'))
    benchmarks = load_json(args.benchmarks_path) if args.benchmarks_path else {}

    perf_col        = step1.get('perf_col')
    pot_col         = step1.get('pot_col')
    perf_thresholds = step1.get('performance', {}).get('thresholds', {})
    pot_thresholds  = step1.get('potential',   {}).get('thresholds', {})

    path = args.input
    df = pd.read_csv(path) if path.endswith('.csv') else pd.read_excel(path)

    if not perf_col or not pot_col or perf_col not in df.columns or pot_col not in df.columns:
        result = {'error': '绩效/潜力列缺失，请检查 step1_precompute.json 中的 perf_col/pot_col'}
        print(json.dumps(result, ensure_ascii=False))
        return

    if '_perf_level' not in df.columns:
        perf_numeric = to_numeric_fallback(df[perf_col])
        df['_perf_level'] = apply_3level(perf_numeric, perf_thresholds)
    if '_pot_level' not in df.columns:
        pot_numeric = to_numeric_fallback(df[pot_col])
        df['_pot_level'] = apply_3level(pot_numeric, pot_thresholds)

    def assign_box(row):
        p, q = row.get('_perf_level'), row.get('_pot_level')
        if pd.notna(p) and pd.notna(q):
            try:
                label, _ = BOX_LABELS.get((int(p), int(q)), ('未分类', ''))
                return label
            except (ValueError, TypeError):
                return '数据异常'
        return '数据缺失'

    df['_9box_label'] = df.apply(assign_box, axis=1)
    df['_9box_label_en'] = df.apply(
        lambda row: BOX_LABELS.get(
            (int(row['_perf_level']), int(row['_pot_level'])), ('', '未分类')
        )[1]
        if pd.notna(row.get('_perf_level')) and pd.notna(row.get('_pot_level')) else '',
        axis=1
    )

    total = len(df)

    # 从 benchmarks.json 读取健康分布参考
    bench_grid = {}
    if benchmarks.get('healthy_distribution', {}).get('grid'):
        for bk, bv in benchmarks['healthy_distribution']['grid'].items():
            zh_label = BENCHMARK_KEY_MAP.get(bk)
            if zh_label:
                bench_grid[zh_label] = {
                    'target_pct': bv.get('target_pct'),
                    'range':      bv.get('range'),
                }

    grid = {}
    for (perf, pot), (label_zh, label_en) in BOX_LABELS.items():
        count = int(((df['_perf_level'] == perf) & (df['_pot_level'] == pot)).sum())
        pct   = round(count / total * 100, 1) if total > 0 else 0
        entry = {
            'count':      count,
            'percentage': pct,
            'perf_level': perf,
            'pot_level':  pot,
            'label_en':   label_en,
        }
        # 附上该格位的健康参考并给出偏差判断
        if label_zh in bench_grid:
            b = bench_grid[label_zh]
            entry['benchmark'] = b
            lo, hi = (b['range'][0] * 100, b['range'][1] * 100) if b.get('range') else (None, None)
            if lo is not None and hi is not None:
                if pct < lo:
                    entry['health_status'] = f'偏低（参考区间 {lo:.0f}%~{hi:.0f}%）'
                elif pct > hi:
                    entry['health_status'] = f'偏高（参考区间 {lo:.0f}%~{hi:.0f}%）'
                else:
                    entry['health_status'] = '正常'
        grid[label_zh] = entry

    # 触发 warning_signals
    triggered_warnings = []
    for ws in benchmarks.get('warning_signals', []):
        triggered_warnings.append({'signal': ws['condition'], 'risk': ws['risk']})

    star_pct  = grid.get('明星人才', {}).get('percentage', 0)
    risk_pct  = sum(grid.get(l, {}).get('percentage', 0) for l in ['需改进者', '观察对象', '待激活者'])
    unclassified = int((df['_9box_label'].isin(['数据缺失', '数据异常', '未分类'])).sum())

    # 实际触发的 warning（基于真实占比）
    actual_warnings = []
    if star_pct < 5:
        actual_warnings.append({'signal': '明星人才 < 5%', 'risk': '高潜人才流失或识别标准过严', 'actual_pct': star_pct})
    low_low_pct = grid.get('需改进者', {}).get('percentage', 0)
    if low_low_pct > 10:
        actual_warnings.append({'signal': '待决策 > 10%', 'risk': '绩效管理或招聘把关存在系统性问题', 'actual_pct': low_low_pct})
    mid_mid_pct = grid.get('稳定贡献者', {}).get('percentage', 0)
    if mid_mid_pct > 40:
        actual_warnings.append({'signal': '中坚力量 > 40%', 'risk': '评估存在趋中效应，区分度不够', 'actual_pct': mid_mid_pct})

    result = {
        'grid':             grid,
        'health': {
            'star_percentage':  star_pct,
            'risk_percentage':  risk_pct,
            'unclassified':     unclassified,
        },
        'actual_warnings':  actual_warnings,
        'total_employees':  total,
    }

    out_json = os.path.join(args.analysis_dir, 'step2_precompute.json')
    with open(out_json, 'w', encoding='utf-8') as f:
        json.dump(result, f, ensure_ascii=False, default=str, indent=2)

    df.to_excel(os.path.join(args.analysis_dir, 'step2_9box_mapping.xlsx'), index=False)
    print(json.dumps(result, ensure_ascii=False, default=str, indent=2))


if __name__ == '__main__':
    main()
