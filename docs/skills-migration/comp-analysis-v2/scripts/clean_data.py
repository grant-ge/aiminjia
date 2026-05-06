#!/usr/bin/env python3
"""
字段检测 + 数据清洗
用法: python3 clean_data.py --input <文件路径> --analysis-dir <中间产物目录>
输出: step1_precompute.json, step1_exclusion_detail.xlsx, step1_cleaned_data.xlsx
"""
import argparse
import json
import os
import sys

import pandas as pd


FIELD_PATTERNS = {
    'id':          ['员工id', 'employee_id', 'emp_id', '工号', 'staff_id', 'id'],
    'name':        ['姓名', '员工姓名', 'name', 'employee_name', 'emp_name'],
    'department':  ['部门', 'department', 'dept'],
    'position':    ['岗位', '职位', 'position', 'job_title', 'title'],
    'level':       ['职级', '层级', 'level', 'grade', 'rank'],
    'status':      ['状态', '在职状态', 'status', 'employment_status'],
    'hire_date':   ['入职日期', '入职时间', 'hire_date', 'start_date', 'join_date'],
    'emp_type':    ['用工类型', '雇佣类型', 'emp_type', 'employment_type'],
    'location':    ['工作地点', '城市', 'location', 'city', 'work_location'],
    'base_salary': ['基本工资', '基础工资', '底薪', 'base_salary', 'base_pay', 'base'],
    'gross':       ['应发工资', '应发合计', '税前工资', 'gross', 'gross_salary', 'gross_pay'],
    'net':         ['实发工资', '实发合计', '税后工资', 'net', 'net_salary', 'net_pay'],
    'bonus':       ['奖金', '绩效奖金', 'bonus', 'incentive'],
    'total_cash':  ['总现金', '总包', '年薪', 'total_cash', 'total_comp', 'ctc'],
    'performance': ['绩效', '绩效评级', '绩效等级', 'performance', 'perf_rating'],
    'gender':      ['性别', 'gender', 'sex'],
}

FIELD_ZH = {
    'id': '员工ID', 'name': '员工姓名', 'department': '部门', 'position': '岗位',
    'level': '职级', 'status': '状态', 'hire_date': '入职日期', 'emp_type': '用工类型',
    'location': '工作地点', 'base_salary': '基本工资', 'gross': '应发工资',
    'net': '实发工资', 'bonus': '奖金', 'total_cash': '总现金',
    'performance': '绩效', 'gender': '性别',
}

EXCLUSION_ZH = {
    'departed': '已离职', 'non_fulltime': '非全职',
    'probation': '试用期', 'current_month_hire': '当月入职',
    'zero_base_salary': '基本工资为0', 'zero_gross': '应发工资为0',
    'null_name': '姓名为空', 'duplicate': '重复记录',
}


def detect_columns(df: pd.DataFrame) -> dict:
    col_lower = {c: c.lower().replace(' ', '').replace('_', '') for c in df.columns}
    detected = {}
    for semantic, patterns in FIELD_PATTERNS.items():
        for col, col_l in col_lower.items():
            for p in patterns:
                p_norm = p.lower().replace(' ', '').replace('_', '')
                if p_norm in col_l or col_l == p_norm:
                    detected[semantic] = col
                    break
            if semantic in detected:
                break
    return detected


def clean_data(df: pd.DataFrame, detected: dict) -> dict:
    original_count = len(df)
    excluded_rows = []

    def mark_excluded(mask, reason):
        for idx in df[mask].index:
            if idx not in [r['_idx'] for r in excluded_rows]:
                row = df.loc[idx].to_dict()
                row['_idx'] = idx
                row['_exclusion_reason'] = EXCLUSION_ZH.get(reason, reason)
                excluded_rows.append(row)

    # 重复记录（按姓名+入职日期去重，或按 ID 去重）
    if 'id' in detected:
        dup_mask = df.duplicated(subset=[detected['id']], keep='first')
        mark_excluded(dup_mask, 'duplicate')
    elif 'name' in detected and 'hire_date' in detected:
        dup_mask = df.duplicated(subset=[detected['name'], detected['hire_date']], keep='first')
        mark_excluded(dup_mask, 'duplicate')

    # 姓名为空
    if 'name' in detected:
        null_mask = df[detected['name']].isna() | (df[detected['name']].astype(str).str.strip() == '')
        mark_excluded(null_mask, 'null_name')

    # 已离职
    if 'status' in detected:
        departed_keywords = ['离职', '已离职', 'terminated', 'resigned', 'left', 'departed']
        departed_mask = df[detected['status']].astype(str).str.strip().str.lower().isin(
            [k.lower() for k in departed_keywords]
        )
        mark_excluded(departed_mask, 'departed')

    # 非全职
    if 'emp_type' in detected:
        non_ft_keywords = ['兼职', '临时', '实习', '劳务', 'part_time', 'part-time', 'temp', 'intern', 'contractor']
        non_ft_mask = df[detected['emp_type']].astype(str).str.strip().str.lower().isin(
            [k.lower() for k in non_ft_keywords]
        )
        mark_excluded(non_ft_mask, 'non_fulltime')

    # 试用期
    if 'status' in detected:
        probation_keywords = ['试用期', '试用', 'probation', 'on probation']
        prob_mask = df[detected['status']].astype(str).str.strip().str.lower().isin(
            [k.lower() for k in probation_keywords]
        )
        mark_excluded(prob_mask, 'probation')

    # 当月入职（hire_date 在当前月）
    if 'hire_date' in detected:
        import datetime
        hire_dates = pd.to_datetime(df[detected['hire_date']], errors='coerce')
        now = datetime.datetime.now()
        current_month_mask = (
            hire_dates.dt.year == now.year
        ) & (hire_dates.dt.month == now.month)
        current_month_mask = current_month_mask.fillna(False)
        mark_excluded(current_month_mask, 'current_month_hire')

    # 基本工资为 0
    if 'base_salary' in detected:
        bs_col = pd.to_numeric(df[detected['base_salary']], errors='coerce')
        zero_mask = bs_col.fillna(0) == 0
        mark_excluded(zero_mask, 'zero_base_salary')

    # 应发工资为 0（仅当没有 base_salary 字段时）
    elif 'gross' in detected:
        gross_col = pd.to_numeric(df[detected['gross']], errors='coerce')
        zero_mask = gross_col.fillna(0) == 0
        mark_excluded(zero_mask, 'zero_gross')

    excluded_indices = list({r['_idx'] for r in excluded_rows})
    retained_df = df.drop(index=excluded_indices).reset_index(drop=True)
    excluded_df = pd.DataFrame([{k: v for k, v in r.items() if k != '_idx'} for r in excluded_rows])

    # 数据质量
    quality = {}
    salary_col = detected.get('base_salary') or detected.get('gross')
    if salary_col:
        sal = pd.to_numeric(retained_df[salary_col], errors='coerce')
        q1, q99 = sal.quantile(0.01), sal.quantile(0.99)
        outliers = int(((sal < q1 * 0.5) | (sal > q99 * 2)).sum())
        quality['salary_outliers'] = outliers
        quality['salary_missing'] = int(sal.isna().sum())

    exclusion_summary = {}
    for r in excluded_rows:
        reason = r.get('_exclusion_reason', '其他')
        exclusion_summary[reason] = exclusion_summary.get(reason, 0) + 1

    return {
        'overview': {'rows': original_count, 'cols': len(df.columns)},
        'retained_df': retained_df,
        'excluded_df': excluded_df,
        'total_retained': len(retained_df),
        'total_excluded': len(excluded_rows),
        'exclusion_summary': exclusion_summary,
        'quality': quality,
    }


def main():
    parser = argparse.ArgumentParser(description='字段检测 + 数据清洗')
    parser.add_argument('--input', required=True, help='输入数据文件路径（xlsx/csv）')
    parser.add_argument('--analysis-dir', required=True, help='中间产物输出目录')
    parser.add_argument(
        '--field-map',
        help='显式字段映射，格式: semantic=列名,semantic=列名（如 base_salary=月度薪酬合计,name=员工姓名）'
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

    # 自动识别后仍缺少关键薪酬字段，输出列名清单让 LLM 引导用户
    if 'base_salary' not in detected and 'gross' not in detected and 'total_cash' not in detected:
        print(json.dumps({
            'status': 'need_field_map',
            'message': '未识别到薪酬字段，请用户从以下列名中指定薪酬列，再通过 --field-map 参数重新运行',
            'columns': list(df.columns),
        }, ensure_ascii=False, indent=2))
        return

    result = clean_data(df, detected)

    # 字段映射表
    field_mapping = [
        {'semantic': sem, 'semantic_zh': FIELD_ZH.get(sem, sem), 'column': col}
        for sem, col in detected.items()
    ]

    precompute = {
        'overview': result['overview'],
        'field_mapping': field_mapping,
        'col_map': detected,
        'total_retained': result['total_retained'],
        'total_excluded': result['total_excluded'],
        'exclusion_details': [
            {'reason': reason, 'count': count}
            for reason, count in result['exclusion_summary'].items()
        ],
        'quality': result['quality'],
    }

    out_json = os.path.join(args.analysis_dir, 'step1_precompute.json')
    with open(out_json, 'w', encoding='utf-8') as f:
        json.dump(precompute, f, ensure_ascii=False, default=str, indent=2)

    if not result['excluded_df'].empty:
        result['excluded_df'].to_excel(
            os.path.join(args.analysis_dir, 'step1_exclusion_detail.xlsx'), index=False
        )
    result['retained_df'].to_excel(
        os.path.join(args.analysis_dir, 'step1_cleaned_data.xlsx'), index=False
    )

    print(json.dumps(precompute, ensure_ascii=False, default=str, indent=2))


if __name__ == '__main__':
    main()
