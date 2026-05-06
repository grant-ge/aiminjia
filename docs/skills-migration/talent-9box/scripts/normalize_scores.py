#!/usr/bin/env python3
"""
绩效/潜力分数归一化
用法: python3 normalize_scores.py --input <文件路径> --analysis-dir <中间产物目录>
输出: step1_precompute.json, step1_normalized_scores.xlsx
"""
import argparse
import json
import os

import pandas as pd


TALENT_FIELD_PATTERNS = {
    'name':              ['姓名', '员工姓名', 'name', 'employee_name'],
    'department':        ['部门', 'department', 'dept'],
    'position':          ['岗位', '职位', 'position', 'job_title'],
    'level':             ['职级', '层级', 'level', 'grade', 'rank'],
    'age':               ['年龄', 'age'],
    'tenure':            ['司龄', '工龄', 'tenure', 'years_of_service', 'seniority'],
    'hire_date':         ['入职日期', 'hire_date', 'start_date'],
    'performance_score': ['绩效分', '绩效得分', '绩效评分', '绩效成绩', 'performance_score', 'performance', 'perf_score', '绩效'],
    'potential_score':   ['潜力分', '潜力得分', '潜力评分', 'potential_score', 'potential', 'pot_score', '潜力'],
    'performance_level': ['绩效等级', '绩效评级', 'performance_level', 'performance_rating', 'perf_rating'],
    'potential_level':   ['潜力等级', '潜力评级', 'potential_level', 'potential_rating', 'pot_rating'],
    'key_position':      ['关键岗位', '核心岗位', 'key_position', 'critical_role'],
    'successor':         ['继任意愿', '继任计划', 'successor', 'succession'],
}

FIELD_ZH = {
    'name': '员工姓名', 'department': '部门', 'position': '岗位', 'level': '职级',
    'age': '年龄', 'tenure': '司龄', 'hire_date': '入职日期',
    'performance_score': '绩效分数', 'potential_score': '潜力分数',
    'performance_level': '绩效等级', 'potential_level': '潜力等级',
    'key_position': '关键岗位', 'successor': '继任意愿',
}

# 文字等级 → 数值映射
LEVEL_MAP = {
    'a': 3, 'b': 2, 'c': 1, 'd': 0,
    '优': 3, '良': 2, '中': 1.5, '差': 1, '不合格': 0,
    '优秀': 3, '良好': 2, '合格': 1, '待改进': 0,
    '高': 3, '中': 2, '低': 1,
    'high': 3, 'medium': 2, 'low': 1,
    'excellent': 3, 'good': 2, 'average': 1, 'poor': 0,
    '5': 5, '4': 4, '3': 3, '2': 2, '1': 1,
}


def detect_columns(df: pd.DataFrame) -> dict:
    col_lower = {c: c.lower().replace(' ', '').replace('_', '') for c in df.columns}
    detected = {}
    for semantic, patterns in TALENT_FIELD_PATTERNS.items():
        for col, col_l in col_lower.items():
            for p in patterns:
                p_norm = p.lower().replace(' ', '').replace('_', '')
                if p_norm in col_l or col_l == p_norm:
                    detected[semantic] = col
                    break
            if semantic in detected:
                break
    return detected


def to_numeric_series(series: pd.Series) -> pd.Series:
    numeric = pd.to_numeric(series, errors='coerce')
    if numeric.notna().sum() > 0:
        return numeric
    return series.astype(str).str.strip().str.lower().map(LEVEL_MAP)


def normalize_to_3level(series: pd.Series, custom_thresholds: dict | None = None) -> tuple[pd.Series, dict]:
    valid = series.dropna()
    if len(valid) == 0:
        return series, {}

    if custom_thresholds:
        q33 = custom_thresholds.get('low_max', float(valid.quantile(0.33)))
        q66 = custom_thresholds.get('mid_max', float(valid.quantile(0.66)))
    else:
        q33 = float(valid.quantile(0.33))
        q66 = float(valid.quantile(0.66))
        # 避免阈值相等（数据高度集中时）
        if q33 == q66:
            q33 = float(valid.quantile(0.25))
            q66 = float(valid.quantile(0.75))

    result = series.apply(
        lambda x: 1 if pd.notna(x) and x <= q33 else (2 if pd.notna(x) and x <= q66 else (3 if pd.notna(x) else x))
    )
    return result, {'low_max': q33, 'mid_max': q66}


def compute_stats(series: pd.Series) -> dict:
    valid = series.dropna()
    if len(valid) == 0:
        return {}
    return {
        'mean':    round(float(valid.mean()), 2),
        'median':  round(float(valid.median()), 2),
        'std':     round(float(valid.std()), 2) if len(valid) > 1 else 0,
        'min':     float(valid.min()),
        'max':     float(valid.max()),
        'count':   int(len(valid)),
        'missing': int(series.isna().sum()),
    }


def main():
    parser = argparse.ArgumentParser(description='绩效/潜力分数归一化')
    parser.add_argument('--input', required=True)
    parser.add_argument('--analysis-dir', required=True)
    parser.add_argument('--perf-low-max', type=float, help='绩效低档上限（用户自定义阈值）')
    parser.add_argument('--perf-mid-max', type=float, help='绩效中档上限（用户自定义阈值）')
    parser.add_argument('--pot-low-max', type=float, help='潜力低档上限（用户自定义阈值）')
    parser.add_argument('--pot-mid-max', type=float, help='潜力中档上限（用户自定义阈值）')
    parser.add_argument(
        '--field-map',
        help='显式字段映射，格式: semantic=列名,semantic=列名（如 performance_score=Q3绩效,potential_score=潜力评级）'
             '。用于自动识别失败时由 LLM 引导用户确认后传入。',
    )
    args = parser.parse_args()

    os.makedirs(args.analysis_dir, exist_ok=True)

    path = args.input
    df = pd.read_csv(path) if path.endswith('.csv') else pd.read_excel(path)

    # 先自动识别，再用显式映射覆盖
    detected = detect_columns(df)
    if args.field_map:
        for pair in args.field_map.split(','):
            if '=' in pair:
                sem, col = pair.strip().split('=', 1)
                sem, col = sem.strip(), col.strip()
                if col in df.columns:
                    detected[sem] = col

    # 自动识别后仍缺少绩效或潜力字段，输出列名清单
    perf_col_check = detected.get('performance_score') or detected.get('performance_level')
    pot_col_check  = detected.get('potential_score')  or detected.get('potential_level')
    if not perf_col_check or not pot_col_check:
        print(json.dumps({
            'status': 'need_field_map',
            'message': f'{"绩效" if not perf_col_check else "潜力"}字段未识别，请用户从以下列名中指定，再通过 --field-map 参数重新运行',
            'columns': list(df.columns),
            'detected_so_far': detected,
        }, ensure_ascii=False, indent=2))
        return

    # 确定绩效/潜力列
    perf_col = detected.get('performance_score') or detected.get('performance_level')
    pot_col  = detected.get('potential_score')  or detected.get('potential_level')

    perf_numeric = to_numeric_series(df[perf_col]) if perf_col else None
    pot_numeric  = to_numeric_series(df[pot_col])  if pot_col  else None

    perf_custom = {}
    if args.perf_low_max is not None:
        perf_custom['low_max'] = args.perf_low_max
    if args.perf_mid_max is not None:
        perf_custom['mid_max'] = args.perf_mid_max

    pot_custom = {}
    if args.pot_low_max is not None:
        pot_custom['low_max'] = args.pot_low_max
    if args.pot_mid_max is not None:
        pot_custom['mid_max'] = args.pot_mid_max

    perf_3level, perf_thresholds = normalize_to_3level(perf_numeric, perf_custom or None) if perf_numeric is not None else (None, {})
    pot_3level,  pot_thresholds  = normalize_to_3level(pot_numeric,  pot_custom  or None) if pot_numeric  is not None else (None, {})

    def level_distribution(series_3level, labels):
        dist = {}
        for val, label in labels.items():
            dist[label] = int((series_3level == val).sum()) if series_3level is not None else 0
        return dist

    perf_labels = {1: '低绩效', 2: '中绩效', 3: '高绩效'}
    pot_labels  = {1: '低潜力', 2: '中潜力', 3: '高潜力'}

    field_mapping = [
        {'semantic': sem, 'semantic_zh': FIELD_ZH.get(sem, sem), 'column': col}
        for sem, col in detected.items()
    ]

    precompute = {
        'field_mapping': field_mapping,
        'col_map': detected,
        'perf_col': perf_col,
        'pot_col':  pot_col,
        'performance': {
            'column':       perf_col,
            'stats':        compute_stats(perf_numeric) if perf_numeric is not None else {},
            'thresholds':   perf_thresholds,
            'distribution': level_distribution(perf_3level, perf_labels),
        },
        'potential': {
            'column':       pot_col,
            'stats':        compute_stats(pot_numeric) if pot_numeric is not None else {},
            'thresholds':   pot_thresholds,
            'distribution': level_distribution(pot_3level, pot_labels),
        },
        'total_employees': len(df),
    }

    out_json = os.path.join(args.analysis_dir, 'step1_precompute.json')
    with open(out_json, 'w', encoding='utf-8') as f:
        json.dump(precompute, f, ensure_ascii=False, default=str, indent=2)

    if perf_3level is not None:
        df['_perf_level'] = perf_3level
    if pot_3level is not None:
        df['_pot_level'] = pot_3level

    df.to_excel(os.path.join(args.analysis_dir, 'step1_normalized_scores.xlsx'), index=False)

    print(json.dumps(precompute, ensure_ascii=False, default=str, indent=2))


if __name__ == '__main__':
    main()
