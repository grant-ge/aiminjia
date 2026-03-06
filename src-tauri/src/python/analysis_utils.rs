//! Pre-written Python analysis utility functions for HR compensation analysis.
//!
//! These functions are injected into every `execute_python` call during analysis mode.
//! The LLM only needs to call these functions rather than writing complex analysis code.

/// Python analysis utility functions (~600 lines).
///
/// Injected after the sandbox preamble and file-loading code, before the LLM's code.
/// Defines: column detection, salary normalization, step-level analysis functions,
/// and statistical helper functions.
///
/// Execution order in analysis mode:
/// ```text
/// [sandbox preamble]       ← security + pandas/numpy/scipy imports + _smart_read_data
/// [file loading]           ← _df = _smart_read_data('uploaded file path')
/// [analysis preamble]      ← save original + restore snapshot + _df_raw + _CURRENT_STEP
/// [ANALYSIS_UTILS]         ← this code: column detection + cleaning + diagnosis + scenarios
/// [LLM code]               ← simple calls: results = _step1_clean(_df)
/// [epilogue]               ← save working snapshot + step snapshot
/// ```
pub const ANALYSIS_UTILS: &str = r###"
# ============================================================
# HR Compensation Analysis Utilities (pre-built)
# ============================================================
# These functions are injected by the system. The LLM should call them
# directly rather than writing analysis code from scratch.

import os as _os
import json as _json
import re as _re
from datetime import datetime as _datetime, timedelta as _timedelta
import math as _math

# ── Analysis results cache directory ──
# _CONV_ID and _ANALYSIS_DIR are injected before this code block.

def _cache_result(step_key, data):
    """Cache step results to JSON for cross-step reference."""
    try:
        _os.makedirs(_ANALYSIS_DIR, exist_ok=True)
        path = _os.path.join(_ANALYSIS_DIR, f'{step_key}_results.json')
        # Convert non-serializable types
        def _convert(obj):
            if isinstance(obj, (np.integer,)):
                return int(obj)
            if isinstance(obj, (np.floating,)):
                return float(obj)
            if isinstance(obj, (np.ndarray,)):
                return obj.tolist()
            if isinstance(obj, pd.Timestamp):
                return obj.isoformat()
            if isinstance(obj, (pd.DataFrame,)):
                return obj.to_dict(orient='records')
            if hasattr(obj, '__dict__'):
                return str(obj)
            return str(obj)
        with open(path, 'w', encoding='utf-8') as f:
            _json.dump(data, f, ensure_ascii=False, indent=2, default=_convert)
    except Exception as e:
        print(f"[cache_result] Warning: failed to cache {step_key}: {e}")

def _load_cached(step_key):
    """Load cached step results. Returns None if not found."""
    try:
        path = _os.path.join(_ANALYSIS_DIR, f'{step_key}_results.json')
        if _os.path.exists(path):
            with open(path, 'r', encoding='utf-8') as f:
                return _json.load(f)
    except Exception:
        pass
    return None

# ============================================================
# 1a. Column Detection
# ============================================================

def _detect_columns(df):
    """Detect column semantics from column names.

    Returns:
        dict with keys:
        - 'detected': {semantic_type: actual_column_name}
        - 'salary_components': {component_type: actual_column_name}
        - 'undetected': [column_names not mapped]
    """
    PATTERNS = {
        'name':       ['姓名', '员工姓名', '名字', 'name', 'employee_name'],
        'id':         ['工号', '员工编号', '员工ID', 'emp_id', 'employee_id', '编号'],
        'department': ['部门', '所属部门', '部门名称', 'department', 'dept', '一级部门', '二级部门'],
        'position':   ['职位', '岗位', '岗位名称', '职位名称', 'position', 'job_title', '职务'],
        'level':      ['职级', '级别', '等级', '岗位等级', 'level', 'grade', '薪等', '薪级'],
        'status':     ['状态', '在职状态', '员工状态', 'status', '人员状态'],
        'hire_date':  ['入职日期', '入职时间', '入司日期', 'hire_date', '入职'],
        'gender':     ['性别', 'gender', 'sex'],
        'location':   ['工作地点', '城市', '地区', 'location', 'city', '工作城市'],
        'emp_type':   ['用工类型', '用工形式', '员工类型', 'employment_type', '用工性质'],
        'base_salary':['基本工资', '基本薪资', '底薪', '岗位工资', 'base_salary', '基薪'],
        'gross':      ['应发工资', '应发合计', '应发薪资', '税前工资', 'gross', '应发总额'],
        'net':        ['实发工资', '实发合计', '到手工资', '税后工资', 'net', '实发总额'],
    }

    SALARY_PATTERNS = {
        'position_allowance': ['岗位津贴', '职务津贴', '职位补贴', '岗位补贴'],
        'performance':        ['绩效工资', '绩效奖金', '绩效', '月度绩效', '绩效考核'],
        'overtime':           ['加班费', '加班工资', '加班'],
        'bonus':              ['奖金', '年终奖', '季度奖', '月度奖金'],
        'allowance':          ['补贴', '津贴', '补助', '餐补', '交通补贴', '住房补贴',
                               '通讯补贴', '高温补贴', '夜班补贴', '餐饮补贴', '交通补助'],
        'deduction':          ['扣款', '扣减', '缺勤扣款', '病假扣款', '事假扣款', '迟到扣款'],
        'commission':         ['提成', '佣金', '业绩提成', '销售提成'],
        'social_insurance':   ['社保', '五险', '社会保险', '养老保险', '医疗保险'],
        'housing_fund':       ['公积金', '住房公积金'],
        'tax':                ['个税', '个人所得税', '代扣个税'],
    }

    cols = list(df.columns)
    col_map = {}
    salary_map = {}
    used = set()

    def _match(col_name, patterns):
        cn = str(col_name).strip()
        # Exact match first
        for p in patterns:
            if cn == p:
                return True
        # Contains match
        for p in patterns:
            if p in cn or cn in p:
                return True
        return False

    # Match standard fields
    for sem_type, patterns in PATTERNS.items():
        for col in cols:
            if col in used:
                continue
            if _match(col, patterns):
                col_map[sem_type] = col
                used.add(col)
                break

    # Match salary components
    for comp_type, patterns in SALARY_PATTERNS.items():
        for col in cols:
            if col in used:
                continue
            if _match(col, patterns):
                salary_map[comp_type] = col
                used.add(col)
                break

    undetected = [c for c in cols if c not in used]

    return {
        'detected': col_map,
        'salary_components': salary_map,
        'undetected': undetected,
    }

# ============================================================
# 1b. Salary Normalization
# ============================================================

def _normalize_salary(series):
    """Normalize salary values: handle 万/K/currency symbols/percentages.

    Converts strings like '1.5万', '15K', '¥8000', '$5000' to float.
    Already-numeric values pass through unchanged.
    """
    def _convert_one(val):
        if pd.isna(val):
            return np.nan
        if isinstance(val, (int, float)):
            return float(val)
        s = str(val).strip()
        # Remove currency symbols
        s = _re.sub(r'[¥￥\$€£₹,，\s]', '', s)
        # Handle 万 (x10000)
        m = _re.match(r'^([\d.]+)\s*万$', s)
        if m:
            return float(m.group(1)) * 10000
        # Handle K (x1000)
        m = _re.match(r'^([\d.]+)\s*[kK]$', s)
        if m:
            return float(m.group(1)) * 1000
        # Handle percentage
        m = _re.match(r'^([\d.]+)\s*%$', s)
        if m:
            return float(m.group(1)) / 100
        # Plain number
        try:
            return float(s)
        except (ValueError, TypeError):
            return np.nan
    return series.apply(_convert_one)

# ============================================================
# 1c. Step 1: Data Cleaning
# ============================================================

def _step1_clean(df, col_map=None):
    """Step 1: Full data cleaning pipeline.

    Args:
        df: Raw DataFrame from uploaded file
        col_map: Optional pre-computed column mapping (from _detect_columns)

    Returns:
        dict with keys:
        - col_map: column mapping result
        - overview: data overview (rows, cols, columns, dtypes)
        - exclusions: dict of exclusion reason -> list of indices
        - exclusion_summary: {reason: count}
        - retained_df: cleaned DataFrame
        - structure: salary structure analysis
        - quality: data quality assessment
    """
    # 0. Column detection
    if col_map is None:
        col_map = _detect_columns(df)
    det = col_map['detected']
    sal_comp = col_map.get('salary_components', {})

    # 1. Data overview
    overview = {
        'rows': len(df),
        'cols': len(df.columns),
        'columns': list(df.columns),
        'dtypes': {col: str(df[col].dtype) for col in df.columns},
    }

    # 2. Normalize salary columns
    salary_cols = [det.get('base_salary'), det.get('gross'), det.get('net')]
    salary_cols = [c for c in salary_cols if c]
    for comp_col in sal_comp.values():
        if comp_col:
            salary_cols.append(comp_col)

    for col in salary_cols:
        if col in df.columns:
            df[col] = _normalize_salary(df[col])

    # 3. Exclusion rules
    exclusions = {}

    # Infer pay period (accounting month) from data
    hire_col = det.get('hire_date')
    if hire_col and hire_col in df.columns:
        df[hire_col] = pd.to_datetime(df[hire_col], errors='coerce')

    # Try to infer accounting period from the data
    _pay_period = None
    if hire_col and hire_col in df.columns:
        max_date = df[hire_col].max()
        if pd.notna(max_date):
            _pay_period = max_date.to_period('M')

    # Rule 1: Departed employees
    status_col = det.get('status')
    if status_col and status_col in df.columns:
        departed_kw = ['离职', '辞退', '解除', '离开', '终止', '退出']
        mask = df[status_col].astype(str).apply(
            lambda x: any(kw in x for kw in departed_kw)
        )
        exclusions['departed'] = df.index[mask].tolist()

    # Rule 2: Non-full-time
    emp_type_col = det.get('emp_type')
    non_ft_kw = ['实习', '劳务派遣', '退休返聘', '兼职', '临时', '外包', '劳务', '顾问']
    if emp_type_col and emp_type_col in df.columns:
        mask = df[emp_type_col].astype(str).apply(
            lambda x: any(kw in x for kw in non_ft_kw)
        )
        already_excluded = set()
        for v in exclusions.values():
            already_excluded.update(v)
        exclusions['non_fulltime'] = [i for i in df.index[mask].tolist() if i not in already_excluded]
    elif status_col and status_col in df.columns:
        mask = df[status_col].astype(str).apply(
            lambda x: any(kw in x for kw in non_ft_kw)
        )
        already_excluded = set()
        for v in exclusions.values():
            already_excluded.update(v)
        exclusions['non_fulltime'] = [i for i in df.index[mask].tolist() if i not in already_excluded]

    # Rule 3: Probation (hired <= 3 months AND status contains 试用)
    if hire_col and hire_col in df.columns and _pay_period:
        three_months_ago = _pay_period.to_timestamp() - _timedelta(days=90)
        probation_mask = (df[hire_col] >= three_months_ago)
        if status_col and status_col in df.columns:
            probation_mask = probation_mask & df[status_col].astype(str).str.contains('试用', na=False)
        else:
            probation_mask = pd.Series(False, index=df.index)
        already_excluded = set()
        for v in exclusions.values():
            already_excluded.update(v)
        exclusions['probation'] = [i for i in df.index[probation_mask].tolist() if i not in already_excluded]

    # Rule 4: Current month hires (incomplete month salary)
    if hire_col and hire_col in df.columns and _pay_period:
        cur_month_mask = df[hire_col].dt.to_period('M') == _pay_period
        already_excluded = set()
        for v in exclusions.values():
            already_excluded.update(v)
        exclusions['current_month_hire'] = [i for i in df.index[cur_month_mask].tolist() if i not in already_excluded]

    # Rule 5: Base salary = 0
    base_col = det.get('base_salary')
    if base_col and base_col in df.columns:
        zero_mask = df[base_col].fillna(0) == 0
        already_excluded = set()
        for v in exclusions.values():
            already_excluded.update(v)
        exclusions['zero_base_salary'] = [i for i in df.index[zero_mask].tolist() if i not in already_excluded]

    # Build exclusion summary
    all_excluded = set()
    exclusion_summary = {}
    for reason, indices in exclusions.items():
        exclusion_summary[reason] = len(indices)
        all_excluded.update(indices)

    # Retained DataFrame
    retained_df = df.drop(index=list(all_excluded))

    # Build excluded DataFrame with reason column (for easy export)
    excl_records = []
    for reason, indices in exclusions.items():
        for idx in indices:
            if idx in df.index:
                row = df.loc[idx].to_dict()
                row['排除原因'] = reason
                excl_records.append(row)
    excluded_df = pd.DataFrame(excl_records) if excl_records else pd.DataFrame()

    # 4. Salary structure analysis
    fixed_cols = []
    variable_cols = []
    fixed_components = ['base_salary', 'position_allowance', 'allowance']
    variable_components = ['performance', 'commission', 'overtime', 'bonus']

    if base_col and base_col in df.columns:
        fixed_cols.append(base_col)
    for comp in fixed_components:
        if comp != 'base_salary' and comp in sal_comp:
            fixed_cols.append(sal_comp[comp])
    for comp in variable_components:
        if comp in sal_comp:
            variable_cols.append(sal_comp[comp])

    structure = {'fixed_cols': fixed_cols, 'variable_cols': variable_cols}
    if fixed_cols or variable_cols:
        rdf = retained_df
        fixed_total = rdf[fixed_cols].fillna(0).sum(axis=1) if fixed_cols else pd.Series(0, index=rdf.index)
        variable_total = rdf[variable_cols].fillna(0).sum(axis=1) if variable_cols else pd.Series(0, index=rdf.index)
        total = fixed_total + variable_total
        total_safe = total.replace(0, np.nan)
        structure['avg_fixed_ratio'] = round(float((fixed_total / total_safe).mean() * 100), 1) if total_safe.notna().any() else None
        structure['avg_variable_ratio'] = round(float((variable_total / total_safe).mean() * 100), 1) if total_safe.notna().any() else None
        structure['median_fixed'] = round(float(fixed_total.median()), 2)
        structure['median_variable'] = round(float(variable_total.median()), 2)

    # 5. Data quality
    quality = {'issues': []}
    key_fields = ['base_salary', 'department', 'position', 'hire_date']
    for field in key_fields:
        col = det.get(field)
        if col and col in df.columns:
            missing_rate = round(float(df[col].isna().mean() * 100), 1)
            if missing_rate > 0:
                quality['issues'].append({
                    'field': field,
                    'column': col,
                    'issue': 'missing',
                    'rate': missing_rate,
                })
        elif field in ['base_salary', 'department']:
            quality['issues'].append({
                'field': field,
                'column': None,
                'issue': 'not_detected',
                'rate': None,
            })

    # Check for negative values in salary columns
    for col in salary_cols:
        if col in retained_df.columns:
            neg_count = int((retained_df[col] < 0).sum())
            if neg_count > 0:
                quality['issues'].append({
                    'field': col,
                    'column': col,
                    'issue': 'negative_values',
                    'count': neg_count,
                })

    result = {
        'col_map': col_map,
        'overview': overview,
        'exclusions': {k: len(v) for k, v in exclusions.items()},
        'exclusion_indices': exclusions,
        'exclusion_summary': exclusion_summary,
        'total_excluded': len(all_excluded),
        'total_retained': len(retained_df),
        'retained_df': retained_df,
        'excluded_df': excluded_df,
        'structure': structure,
        'quality': quality,
    }

    # Cache (without DataFrames)
    cache_data = {k: v for k, v in result.items() if k not in ('retained_df', 'excluded_df', 'exclusion_indices')}
    _cache_result('step1', cache_data)

    # Auto-update global _df (LLM-proof: no manual assignment needed)
    try:
        globals()['_df'] = retained_df
    except Exception:
        pass

    # Display results
    print("=== Data Overview ===")
    print(f"  Rows: {overview['rows']}, Columns: {overview['cols']}")
    print(f"\n=== Exclusion Summary (excluded {len(all_excluded)}, retained {len(retained_df)}) ===")
    for reason, count in exclusion_summary.items():
        if count > 0:
            print(f"  {reason}: {count}")
    print(f"\n=== Salary Structure ===")
    if structure.get('avg_fixed_ratio'):
        print(f"  Fixed ratio: {structure['avg_fixed_ratio']}%")
        print(f"  Variable ratio: {structure['avg_variable_ratio']}%")
    print(f"\n=== Data Quality ({len(quality['issues'])} issues) ===")
    for iss in quality['issues']:
        print(f"  {iss['field']}: {iss['issue']} ({iss.get('rate', iss.get('count', 'N/A'))})")
    print(f"\n=== Column Mapping ===")
    for sem, col in det.items():
        print(f"  {sem} -> {col}")

    return result

# ============================================================
# 1b-2. Step 2: Position Normalization & Job Family Construction
# ============================================================

_INDUSTRY_TEMPLATES = {
    ('manufacturing', 'large'):   ['tech_rd', 'sales_marketing', 'customer_service', 'production', 'supply_chain', 'support', 'quality', 'logistics'],
    ('manufacturing', 'medium'):  ['tech', 'business', 'production', 'supply_chain', 'support', 'logistics'],
    ('manufacturing', 'small'):   ['business', 'production', 'support', 'management'],
    ('internet', 'large'):        ['rd', 'product', 'design', 'operations', 'sales', 'support', 'management'],
    ('internet', 'medium'):       ['tech', 'design', 'operations', 'business', 'support'],
    ('internet', 'small'):        ['tech', 'business', 'support'],
    ('finance', 'large'):         ['front_office', 'mid_office', 'back_office', 'tech', 'compliance', 'support', 'management'],
    ('general', 'default'):       ['tech', 'business', 'operations', 'support', 'management'],
}

_FAMILY_NAMES_ZH = {
    'tech_rd': '技术研发', 'sales_marketing': '销售营销', 'customer_service': '客户服务',
    'production': '生产制造', 'supply_chain': '供应链', 'support': '职能支持',
    'quality': '品质', 'logistics': '后勤保障', 'tech': '技术', 'business': '商务',
    'management': '管理', 'rd': '研发', 'product': '产品', 'design': '设计',
    'operations': '运营', 'sales': '销售', 'front_office': '前台业务',
    'mid_office': '中台风控', 'back_office': '后台运营', 'compliance': '合规法务',
}

def _infer_industry(df, col_map):
    """Infer industry type and company scale from data signals."""
    det = col_map.get('detected', {})
    dept_col = det.get('department')
    pos_col = det.get('position')
    n = len(df)

    signals = []
    industry_scores = {'manufacturing': 0, 'internet': 0, 'finance': 0}

    if dept_col and dept_col in df.columns:
        dept_text = ' '.join(df[dept_col].dropna().astype(str).unique())
        for kw in ['车间', '生产', '装配', '制造', '工艺', '质检', '模具', '仓储']:
            if kw in dept_text:
                industry_scores['manufacturing'] += 1
                signals.append(f'dept:{kw}')
        for kw in ['研发', '产品', '运营', '技术', '开发', '测试', 'IT', '算法']:
            if kw in dept_text:
                industry_scores['internet'] += 1
                signals.append(f'dept:{kw}')
        for kw in ['风控', '合规', '投资', '信贷', '资管', '理财', '信托']:
            if kw in dept_text:
                industry_scores['finance'] += 1
                signals.append(f'dept:{kw}')

    if pos_col and pos_col in df.columns:
        pos_text = ' '.join(df[pos_col].dropna().astype(str).unique())
        if any(kw in pos_text for kw in ['操作工', '装配工', '焊工', '钳工', '车工']):
            industry_scores['manufacturing'] += 3
            signals.append('pos:blue_collar')
        if any(kw in pos_text for kw in ['工程师', '架构师', '前端', '后端', 'DevOps']):
            industry_scores['internet'] += 2
            signals.append('pos:tech_roles')
        if any(kw in pos_text for kw in ['外贸', '国际', '跟单']):
            industry_scores['manufacturing'] += 1
            signals.append('pos:trade')
        blue_kw = ['操作', '生产', '装配', '仓库', '物流', '司机', '搬运', '焊', '钳', '车工']
        blue_count = df[pos_col].astype(str).apply(lambda x: any(kw in x for kw in blue_kw)).sum()
        blue_ratio = blue_count / max(n, 1)
        if blue_ratio > 0.3:
            industry_scores['manufacturing'] += 3
            signals.append(f'blue_collar_ratio:{blue_ratio:.0%}')

    best = max(industry_scores, key=industry_scores.get)
    if industry_scores[best] == 0:
        best = 'general'

    scale = 'large' if n >= 500 else ('medium' if n >= 100 else 'small')
    signals.append(f'headcount:{n}->scale:{scale}')

    return {'industry': best, 'scale': scale, 'signals': signals}


def _recommend_job_families(industry_info):
    """Recommend 2-3 job family schemes based on industry inference."""
    ind = industry_info['industry']
    scale = industry_info['scale']
    schemes = []

    key = (ind, scale)
    if key not in _INDUSTRY_TEMPLATES:
        key = (ind, 'default') if (ind, 'default') in _INDUSTRY_TEMPLATES else ('general', 'default')
    families = _INDUSTRY_TEMPLATES[key]
    schemes.append({
        'name': f'{ind}_{scale}',
        'families': families,
        'families_zh': [_FAMILY_NAMES_ZH.get(f, f) for f in families],
        'count': len(families),
        'match_score': 95,
    })

    alt_scale = 'medium' if scale == 'large' else ('large' if scale == 'small' else 'small')
    alt_key = (ind, alt_scale)
    if alt_key in _INDUSTRY_TEMPLATES:
        alt_families = _INDUSTRY_TEMPLATES[alt_key]
        schemes.append({
            'name': f'{ind}_{alt_scale}',
            'families': alt_families,
            'families_zh': [_FAMILY_NAMES_ZH.get(f, f) for f in alt_families],
            'count': len(alt_families),
            'match_score': 70,
        })

    gen_families = _INDUSTRY_TEMPLATES[('general', 'default')]
    schemes.append({
        'name': 'general',
        'families': gen_families,
        'families_zh': [_FAMILY_NAMES_ZH.get(f, f) for f in gen_families],
        'count': len(gen_families),
        'match_score': 50,
    })

    return schemes


def _normalize_positions(df, col_map, job_families_zh):
    """Normalize raw position names and assign to job families."""
    det = col_map.get('detected', {})
    pos_col = det.get('position')
    dept_col = det.get('department')
    salary_col = det.get('base_salary') or det.get('gross')

    if not pos_col or pos_col not in df.columns:
        return {'error': 'No position column found'}

    _FAMILY_KEYWORDS = {
        '技术研发': ['工程师', '开发', '架构', '算法', '测试', 'QA', '技术', '研发'],
        '技术': ['工程师', '开发', '架构', '算法', '测试', 'QA', '技术', '研发'],
        '研发': ['工程师', '开发', '架构', '算法', '研发', '实验'],
        '销售营销': ['销售', '业务', 'BD', '营销', '市场', '推广', '渠道'],
        '商务': ['销售', '业务', 'BD', '营销', '市场', '推广', '商务'],
        '销售': ['销售', '业务', 'BD', '渠道'],
        '客户服务': ['客服', '售后', '服务', '支持'],
        '生产制造': ['操作', '生产', '装配', '制造', '焊', '钳', '车工', '模具', '工艺', '质检'],
        '生产': ['操作', '生产', '装配', '制造', '焊', '钳', '车工'],
        '供应链': ['采购', '供应', '物流', '仓储', '仓库', '计划'],
        '职能支持': ['人事', 'HR', '财务', '行政', '法务', '审计', '总务', '文员'],
        '支持': ['人事', 'HR', '财务', '行政', '法务', '审计', '总务', '文员'],
        '品质': ['品质', '质量', 'QC', '检验', '体系'],
        '后勤保障': ['保安', '司机', '清洁', '食堂', '宿管', '维修', '保洁'],
        '管理': ['总经理', '副总', '总监', '经理', '主管', '厂长', '主任'],
        '产品': ['产品', 'PM', '需求', '规划'],
        '设计': ['设计', 'UI', 'UX', '美术', '视觉'],
        '运营': ['运营', '内容', '社区', '活动', '增长'],
        '前台业务': ['客户经理', '投顾', '理财', '信贷'],
        '中台风控': ['风控', '风险', '合规', '审批'],
        '后台运营': ['清算', '结算', '运营', '系统'],
        '合规法务': ['合规', '法务', '反洗钱', '审计'],
    }

    active_keywords = {}
    for fam in job_families_zh:
        if fam in _FAMILY_KEYWORDS:
            active_keywords[fam] = _FAMILY_KEYWORDS[fam]

    mgmt_keywords = ['总经理', '副总', '总监', '厂长']
    raw_positions = df[pos_col].dropna().unique()
    mappings = []

    for raw in raw_positions:
        raw_str = str(raw).strip()
        count = int((df[pos_col] == raw).sum())

        standard = _re.sub(r'^(国际|海外|国内|华东|华南|华北|北方|南方|东区|西区)\s*', '', raw_str)
        standard = _re.sub(r'\s*[A-D]\d*$', '', standard)
        standard = _re.sub(r'\s*[一二三四五六七八九十]+级$', '', standard)

        best_family = None
        best_score = 0
        for fam, keywords in active_keywords.items():
            score = sum(1 for kw in keywords if kw in raw_str)
            if dept_col and dept_col in df.columns:
                dept_vals = df[df[pos_col] == raw][dept_col].dropna().unique()
                for dv in dept_vals:
                    score += sum(0.5 for kw in keywords if kw in str(dv))
            if score > best_score:
                best_score = score
                best_family = fam

        is_manager = any(kw in raw_str for kw in mgmt_keywords)

        if best_family is None:
            if is_manager and '管理' in job_families_zh:
                best_family = '管理'
                best_score = 1
            else:
                best_family = job_families_zh[-1]
                best_score = 0

        confidence = min(1.0, best_score / 2.0) if best_score > 0 else 0.3

        mappings.append({
            'raw': raw_str,
            'standard': standard,
            'family': best_family,
            'count': count,
            'confidence': round(confidence, 2),
            'is_manager': is_manager,
        })

    mappings.sort(key=lambda m: (m['family'], -m['count']))

    family_dist = {}
    for m in mappings:
        family_dist[m['family']] = family_dist.get(m['family'], 0) + m['count']

    low_conf = [m for m in mappings if m['confidence'] < 0.7]

    validation_notes = []
    if salary_col and salary_col in df.columns:
        for fam in job_families_zh:
            fam_positions = [m['raw'] for m in mappings if m['family'] == fam]
            if len(fam_positions) < 2:
                continue
            medians = []
            for pos in fam_positions:
                s = df[df[pos_col] == pos][salary_col].dropna()
                if len(s) > 0:
                    medians.append(float(s.median()))
            if len(medians) >= 2:
                cv = round(float(np.std(medians) / np.mean(medians) * 100), 1) if np.mean(medians) > 0 else 0
                if cv > 50:
                    validation_notes.append(f'{fam}: salary CV={cv}% across positions, may need sub-grouping')

    # Auto-apply columns to _df
    pos_to_std = {m['raw']: m['standard'] for m in mappings}
    pos_to_fam = {m['raw']: m['family'] for m in mappings}
    try:
        gdf = globals().get('_df')
        if gdf is not None and pos_col in gdf.columns:
            gdf['standard_position'] = gdf[pos_col].map(pos_to_std).fillna(gdf[pos_col])
            gdf['job_family'] = gdf[pos_col].map(pos_to_fam).fillna('unknown')
    except Exception:
        pass

    result = {
        'mapping': mappings,
        'low_confidence': low_conf,
        'family_distribution': family_dist,
        'validation_notes': validation_notes,
    }
    _cache_result('step2', result)
    return result


def _step2_normalize(df, col_map=None, scheme_index=0):
    """Step 2: Full position normalization pipeline.

    One-call function: industry inference + scheme recommendation +
    position normalization + salary validation.

    Args:
        df: Cleaned DataFrame (from step1)
        col_map: Column mapping (auto-loads from step1 cache if None)
        scheme_index: Which recommended scheme to use (0=best match)

    Returns:
        dict with 'industry', 'schemes', 'selected_scheme', 'normalization'
    """
    if col_map is None:
        step1 = _load_cached('step1')
        if step1:
            col_map = step1.get('col_map')
    if col_map is None:
        col_map = _detect_columns(df)

    industry = _infer_industry(df, col_map)
    schemes = _recommend_job_families(industry)
    idx = min(scheme_index, len(schemes) - 1)
    selected = schemes[idx]
    norm = _normalize_positions(df, col_map, selected['families_zh'])

    # Display
    print(f"=== Industry Inference ===")
    print(f"  Type: {industry['industry']}, Scale: {industry['scale']} ({len(df)} people)")
    print(f"  Signals: {', '.join(industry['signals'][:5])}")
    print(f"\n=== Recommended Schemes ===")
    for i, s in enumerate(schemes):
        marker = ' <<< selected' if i == idx else ''
        print(f"  Scheme {i+1} ({s['name']}): {s['count']} families - {', '.join(s['families_zh'])} [{s['match_score']}%]{marker}")
    print(f"\n=== Position Normalization ===")
    current_family = None
    for m in norm.get('mapping', []):
        if m['family'] != current_family:
            current_family = m['family']
            fam_count = norm.get('family_distribution', {}).get(current_family, 0)
            print(f"\n  [{current_family}] ({fam_count} people)")
        conf_marker = ' [?]' if m['confidence'] < 0.7 else ''
        print(f"    {m['raw']} -> {m['standard']} ({m['count']}p, {m['confidence']:.0%}){conf_marker}")
    low_conf = norm.get('low_confidence', [])
    if low_conf:
        print(f"\n=== Low Confidence ({len(low_conf)} items) ===")
        for m in low_conf:
            print(f"  {m['raw']} -> suggested: {m['family']} ({m['confidence']:.0%})")
    val_notes = norm.get('validation_notes', [])
    if val_notes:
        print(f"\n=== Salary Validation Notes ===")
        for note in val_notes:
            print(f"  {note}")
    print(f"\n=== Family Distribution ===")
    for fam in selected['families_zh']:
        cnt = norm.get('family_distribution', {}).get(fam, 0)
        pct = round(cnt / max(len(df), 1) * 100, 1)
        print(f"  {fam}: {cnt} ({pct}%)")

    result = {
        'industry': industry,
        'schemes': schemes,
        'selected_scheme': selected,
        'normalization': norm,
    }
    return result

# ============================================================
# 1b-3. Step 3: Level Inference & Grading
# ============================================================

_LEVEL_TEMPLATES = {
    ('manufacturing', 'large'):  {'P': 7, 'S': 5, 'O': 4, 'M': 4},
    ('internet', 'large'):       {'P': 10, 'M': 6},
    ('internet', 'medium'):      {'P': 8, 'M': 5},
    ('manufacturing', 'medium'): {'T': 6, 'B': 5, 'M': 4},
    ('general', 'default'):      {'L': 8},
}

_TRACK_NAMES_ZH = {
    'P': '专业序列', 'S': '销售序列', 'O': '操作序列', 'M': '管理序列',
    'T': '技术序列', 'B': '商务序列', 'L': '通用序列',
}

def _recommend_level_scheme(industry_info):
    """Recommend level/track schemes based on industry inference."""
    ind = industry_info['industry']
    scale = industry_info['scale']
    schemes = []

    key = (ind, scale)
    if key not in _LEVEL_TEMPLATES:
        key = (ind, 'medium') if (ind, 'medium') in _LEVEL_TEMPLATES else ('general', 'default')
    tracks = _LEVEL_TEMPLATES[key]
    total = sum(tracks.values())
    desc = ' / '.join(f'{_TRACK_NAMES_ZH.get(t,t)}{n}级' for t, n in tracks.items())
    schemes.append({
        'name': f'{ind}_{scale}', 'tracks': tracks,
        'tracks_zh': {_TRACK_NAMES_ZH.get(t,t): n for t, n in tracks.items()},
        'total_levels': total, 'description': desc, 'match_score': 90,
    })
    if total > 8:
        schemes.append({
            'name': 'simplified', 'tracks': {'L': 8},
            'tracks_zh': {'通用序列': 8},
            'total_levels': 8, 'description': '通用序列8级', 'match_score': 50,
        })
    return schemes


def _infer_crude_level(df, col_map, track_scheme):
    """Phase A: Infer crude level from non-salary signals (simplified IPE)."""
    det = col_map.get('detected', {})
    pos_col = det.get('position')
    dept_col = det.get('department')
    level_col = det.get('level')
    hire_col = det.get('hire_date')
    tracks = track_scheme['tracks']

    # Use existing level column if available
    if level_col and level_col in df.columns:
        existing = df[level_col].dropna().unique()
        if len(existing) >= 3:
            return df[level_col].copy(), 'existing_column'

    rdf = df.copy()
    track_labels = list(tracks.keys())

    # Score each person (0-100)
    scores = pd.Series(50.0, index=rdf.index)

    if pos_col and pos_col in rdf.columns:
        pos_str = rdf[pos_col].astype(str)
        scores += pos_str.apply(lambda x: 30 if any(kw in x for kw in ['总经理', '副总', 'CEO', 'VP', 'CTO', 'CFO']) else 0)
        scores += pos_str.apply(lambda x: 20 if any(kw in x for kw in ['总监', '厂长', '部长']) else 0)
        scores += pos_str.apply(lambda x: 10 if any(kw in x for kw in ['经理', '主管', '主任', '组长', '班长']) else 0)
        scores += pos_str.apply(lambda x: 5 if any(kw in x for kw in ['高级', '资深', '首席', 'senior', 'lead']) else 0)
        scores += pos_str.apply(lambda x: -10 if any(kw in x for kw in ['助理', '实习', '初级', '学徒', 'junior']) else 0)

    if dept_col and dept_col in rdf.columns and pos_col and pos_col in rdf.columns:
        dept_sizes = rdf[dept_col].map(rdf[dept_col].value_counts())
        is_mgr = rdf[pos_col].astype(str).apply(
            lambda x: any(kw in x for kw in ['经理', '总监', '主管', '主任', '部长'])
        )
        scores += (dept_sizes / dept_sizes.max() * 10 * is_mgr.astype(int))

    if hire_col and hire_col in rdf.columns:
        tenure = _tenure_years(rdf, hire_col)
        scores += tenure.clip(0, 15)

    # Determine track per person
    if len(track_labels) > 1:
        job_family_col = 'job_family' if 'job_family' in rdf.columns else None
        track_map = {}
        if 'O' in tracks:
            track_map.update({'生产制造': 'O', '生产': 'O', '后勤保障': 'O'})
        if 'S' in tracks:
            track_map.update({'销售营销': 'S', '销售': 'S', '商务': 'S'})
        if 'P' in tracks:
            track_map.update({'技术研发': 'P', '技术': 'P', '研发': 'P', '产品': 'P',
                              '设计': 'P', '职能支持': 'P', '支持': 'P', '品质': 'P',
                              '运营': 'P', '客户服务': 'P', '供应链': 'P'})
        if 'T' in tracks:
            track_map.update({'技术研发': 'T', '技术': 'T', '研发': 'T'})
        if 'B' in tracks:
            track_map.update({'销售营销': 'B', '销售': 'B', '商务': 'B', '运营': 'B',
                              '客户服务': 'B', '供应链': 'B'})
        if 'M' in tracks:
            track_map.update({'管理': 'M'})
        default_track = track_labels[0]
        if job_family_col:
            rdf['_track'] = rdf[job_family_col].map(track_map).fillna(default_track)
        else:
            rdf['_track'] = default_track
        if 'M' in tracks and pos_col and pos_col in rdf.columns:
            senior_mgmt = rdf[pos_col].astype(str).apply(
                lambda x: any(kw in x for kw in ['总经理', '副总', '总监', '厂长'])
            )
            rdf.loc[senior_mgmt, '_track'] = 'M'
    else:
        rdf['_track'] = track_labels[0]

    # Map scores to levels via quantile binning
    levels = pd.Series(index=rdf.index, dtype=str)
    for track in tracks:
        mask = rdf['_track'] == track
        if not mask.any():
            continue
        track_scores = scores[mask]
        n_levels = tracks[track]
        try:
            bins = pd.qcut(track_scores, q=min(n_levels, len(track_scores.unique())),
                           labels=False, duplicates='drop')
            levels[mask] = bins.apply(lambda x: f'{track}{int(x)+1}')
        except Exception:
            normalized = ((track_scores - track_scores.min()) / max(track_scores.max() - track_scores.min(), 1)).clip(0, 0.999)
            levels[mask] = normalized.apply(lambda x: f'{track}{max(1, min(n_levels, int(x * n_levels) + 1))}')

    return levels, 'inferred'


def _salary_cluster_sublevel(df, salary_col, level_col):
    """Phase B: Sub-level refinement via salary clustering."""
    refined = df[level_col].copy()
    for group_level in df[level_col].dropna().unique():
        mask = df[level_col] == group_level
        group_salaries = df.loc[mask, salary_col].dropna()
        if len(group_salaries) < 6:
            continue
        best_k = 3 if len(group_salaries) >= 12 else 2
        try:
            from sklearn.cluster import KMeans
            vals = group_salaries.values.reshape(-1, 1)
            km = KMeans(n_clusters=best_k, random_state=42, n_init=10)
            labels = km.fit_predict(vals)
            centroids = km.cluster_centers_.flatten()
            order = np.argsort(centroids)
            sublevel_map = {old: chr(97 + new) for new, old in enumerate(order)}
            sublabels = pd.Series(labels, index=group_salaries.index).map(sublevel_map)
            refined.loc[group_salaries.index] = group_level + sublabels
        except Exception:
            pass
    return refined


def _cross_validate_levels(df, salary_col, level_col, hire_col=None):
    """Phase C: Cross-validation - flag level-salary inconsistencies."""
    anomalies = []
    level_medians = df.groupby(level_col)[salary_col].median().sort_index()
    sorted_levels = level_medians.index.tolist()
    for i in range(1, len(sorted_levels)):
        if level_medians.iloc[i] < level_medians.iloc[i-1]:
            anomalies.append({
                'type': 'level_salary_inversion',
                'detail': f'{sorted_levels[i]} median ({level_medians.iloc[i]:.0f}) < {sorted_levels[i-1]} ({level_medians.iloc[i-1]:.0f})',
                'severity': 'high',
            })
    if hire_col and hire_col in df.columns:
        tenure = _tenure_years(df, hire_col)
        for level in df[level_col].dropna().unique():
            mask = df[level_col] == level
            group = df[mask].copy()
            group['_tenure'] = tenure[mask]
            if len(group) < 4:
                continue
            median_sal = group[salary_col].median()
            median_tenure = group['_tenure'].median()
            for idx in group[(group['_tenure'] > median_tenure * 1.5) & (group[salary_col] < median_sal * 0.85)].index:
                anomalies.append({
                    'type': 'suspected_underpaid', 'index': int(idx), 'level': str(level),
                    'salary': round(float(group.loc[idx, salary_col]), 0),
                    'tenure': round(float(group.loc[idx, '_tenure']), 1),
                    'detail': f'High tenure but low salary', 'severity': 'medium',
                })
            for idx in group[(group['_tenure'] < median_tenure * 0.5) & (group[salary_col] > median_sal * 1.2)].index:
                anomalies.append({
                    'type': 'suspected_overpaid', 'index': int(idx), 'level': str(level),
                    'salary': round(float(group.loc[idx, salary_col]), 0),
                    'tenure': round(float(group.loc[idx, '_tenure']), 1),
                    'detail': f'Low tenure but high salary', 'severity': 'medium',
                })
    return anomalies


def _validate_step3(df, level_col, salary_col):
    """Built-in validation for Step 3 results."""
    issues = []
    lines = ['=== Step 3 Validation ===']

    dist = df[level_col].value_counts().sort_index()
    lines.append('\n1. Level distribution:')
    for lvl, cnt in dist.items():
        lines.append(f'   {lvl}: {cnt}')

    lines.append('\n2. Salary-level monotonicity:')
    medians = df.groupby(level_col)[salary_col].median().sort_index()
    prev_med, prev_lbl = None, None
    for lvl, med in medians.items():
        lines.append(f'   {lvl}: median={med:.0f}')
        if prev_med is not None and med < prev_med:
            issues.append(f'Salary inversion: {lvl} ({med:.0f}) < {prev_lbl} ({prev_med:.0f})')
        prev_med, prev_lbl = med, lvl

    lines.append('\n3. Per-level CV:')
    for lvl in df[level_col].dropna().unique():
        s = df[df[level_col] == lvl][salary_col].dropna()
        if len(s) >= 2:
            cv = round(float(s.std() / s.mean() * 100), 1) if s.mean() > 0 else 0
            lines.append(f'   {lvl}: CV={cv}% (n={len(s)})')
            if cv > 30:
                issues.append(f'{lvl}: CV={cv}% > 30%')

    singles = [str(lvl) for lvl, cnt in dist.items() if cnt == 1]
    if singles:
        lines.append(f'\n4. Single-person levels: {", ".join(singles)}')

    passed = len(issues) == 0
    lines.append(f'\n=== {"PASS" if passed else f"FAIL ({len(issues)} issues)"} ===')
    for iss in issues:
        lines.append(f'  ! {iss}')
    display = '\n'.join(lines)
    print(display)
    return {'passed': passed, 'issues': issues, 'display': display}


def _step3_grading(df, col_map=None, scheme_index=0, enable_sublevel=True):
    """Step 3: Full level inference pipeline.

    Three phases: A) Non-salary -> crude level, B) Salary clustering -> sub-level, C) Cross-validation.
    """
    if col_map is None:
        step1 = _load_cached('step1')
        if step1:
            col_map = step1.get('col_map')
    if col_map is None:
        col_map = _detect_columns(df)

    step2 = _load_cached('step2')
    industry_info = step2.get('industry') if step2 and isinstance(step2, dict) else None
    if not industry_info:
        industry_info = _infer_industry(df, col_map)

    det = col_map.get('detected', {})
    salary_col = det.get('base_salary') or det.get('gross')
    hire_col = det.get('hire_date')
    level_col = det.get('level')
    pos_col = det.get('position')

    schemes = _recommend_level_scheme(industry_info)
    idx = min(scheme_index, len(schemes) - 1)
    selected = schemes[idx]

    levels, source = _infer_crude_level(df, col_map, selected)
    df = df.copy()
    if source == 'existing_column' and level_col:
        level_col_name = level_col
    else:
        level_col_name = '_inferred_level'
        df[level_col_name] = levels

    active_level_col = level_col_name
    if enable_sublevel and salary_col and salary_col in df.columns:
        try:
            refined = _salary_cluster_sublevel(df, salary_col, level_col_name)
            df['_refined_level'] = refined
            active_level_col = '_refined_level'
        except Exception:
            pass

    anomalies = _cross_validate_levels(df, salary_col, active_level_col, hire_col) if salary_col and salary_col in df.columns else []
    validation = _validate_step3(df, active_level_col, salary_col) if salary_col and salary_col in df.columns else {'passed': True, 'issues': []}

    # Auto-apply
    try:
        gdf = globals().get('_df')
        if gdf is not None:
            gdf['inferred_level'] = df[active_level_col]
            if col_map and isinstance(col_map, dict) and 'detected' in col_map:
                col_map['detected']['level'] = 'inferred_level'
    except Exception:
        pass

    # Display
    print('=== Level Scheme ===')
    for i, s in enumerate(schemes):
        marker = ' <<< selected' if i == idx else ''
        print(f"  Scheme {i+1}: {s['description']} ({s['total_levels']} levels) [{s['match_score']}%]{marker}")
    print(f"\n=== Grading Results (source: {source}) ===")
    jf_col = 'job_family' if 'job_family' in df.columns else (pos_col if pos_col else None)
    if jf_col and jf_col in df.columns:
        for jf in sorted(df[jf_col].dropna().unique()):
            jf_data = df[df[jf_col] == jf]
            level_dist = jf_data[active_level_col].value_counts().sort_index()
            avg_sal = round(float(jf_data[salary_col].mean()), 0) if salary_col and salary_col in jf_data.columns else 'N/A'
            print(f"\n  [{jf}] ({len(jf_data)}p, avg: {avg_sal})")
            for lvl, cnt in level_dist.items():
                print(f"    {lvl}: {cnt}")
    if anomalies:
        print(f"\n=== Anomalies ({len(anomalies)}) ===")
        by_type = {}
        for a in anomalies:
            by_type[a['type']] = by_type.get(a['type'], 0) + 1
        for t, cnt in by_type.items():
            print(f"  {t}: {cnt}")

    result = {
        'scheme': selected, 'schemes': schemes, 'source': source,
        'level_column': active_level_col,
        'anomalies': anomalies, 'anomaly_count': len(anomalies),
        'validation': validation,
    }
    cache_data = {k: v for k, v in result.items()}
    _cache_result('step3', cache_data)
    return result

# ============================================================
# Statistical Helpers
# ============================================================

def _calc_cv(series):
    """Coefficient of Variation (CV) = std / mean * 100."""
    s = series.dropna()
    if len(s) < 2 or s.mean() == 0:
        return None
    return round(float(s.std() / s.mean() * 100), 2)

def _calc_gini(series):
    """Gini coefficient (0 = perfect equality, 1 = maximum inequality)."""
    s = series.dropna().values.astype(float)
    s = s[s > 0]
    if len(s) < 2:
        return None
    s = np.sort(s)
    n = len(s)
    index = np.arange(1, n + 1)
    return round(float((2 * np.sum(index * s) - (n + 1) * np.sum(s)) / (n * np.sum(s))), 4)

def _calc_compa_ratio(salary, group_median):
    """Compa-Ratio = salary / group_median * 100."""
    if pd.isna(salary) or pd.isna(group_median) or group_median == 0:
        return np.nan
    return round(float(salary / group_median * 100), 1)

def _detect_outliers_iqr(series, threshold=1.5):
    """Detect outliers using IQR method.

    Returns: dict with 'lower_bound', 'upper_bound', 'outlier_indices'
    """
    s = series.dropna()
    if len(s) < 4:
        return {'lower_bound': None, 'upper_bound': None, 'outlier_indices': []}
    q1 = float(s.quantile(0.25))
    q3 = float(s.quantile(0.75))
    iqr = q3 - q1
    lower = q1 - threshold * iqr
    upper = q3 + threshold * iqr
    outliers = series[(series < lower) | (series > upper)].index.tolist()
    return {
        'lower_bound': round(lower, 2),
        'upper_bound': round(upper, 2),
        'outlier_indices': outliers,
        'count': len(outliers),
    }

def _salary_stats(df, salary_col, group_col):
    """Group-level salary statistics.

    Returns DataFrame with: group, count, mean, median, min, max, std, cv
    """
    stats = df.groupby(group_col)[salary_col].agg(
        ['count', 'mean', 'median', 'min', 'max', 'std']
    ).reset_index()
    stats.columns = [group_col, 'count', 'mean', 'median', 'min', 'max', 'std']
    stats['cv'] = round(stats['std'] / stats['mean'] * 100, 2)
    stats['range_ratio'] = round(stats['max'] / stats['min'].replace(0, np.nan), 2)
    # Round numeric columns
    for c in ['mean', 'median', 'min', 'max', 'std']:
        stats[c] = stats[c].round(0)
    return stats

def _tenure_years(df, hire_col, reference_date=None):
    """Calculate tenure in years from hire date.

    Returns Series of tenure in years (float).
    """
    if reference_date is None:
        reference_date = pd.Timestamp.now()
    hire_dates = pd.to_datetime(df[hire_col], errors='coerce')
    tenure = (reference_date - hire_dates).dt.days / 365.25
    return tenure.round(2)

# ============================================================
# 1d. Step 4: Six-Dimension Fairness Diagnosis
# ============================================================

def _dim1_internal_equity(df, salary_col, group_cols):
    """Dimension 1: Internal equity within position x level groups.

    Calculates CV, range ratio, IQR for each group.
    Flags groups with CV > 20% or range_ratio > 2.0.
    """
    if not group_cols or not salary_col:
        return {'error': 'Missing salary or group columns', 'groups': []}

    # Build composite group key
    valid_cols = [c for c in group_cols if c and c in df.columns]
    if not valid_cols:
        return {'error': 'No valid group columns found', 'groups': []}

    df = df.copy()
    df['_group'] = df[valid_cols].astype(str).agg(' × '.join, axis=1)

    results = []
    for group_name, gdf in df.groupby('_group'):
        s = gdf[salary_col].dropna()
        if len(s) < 2:
            continue
        cv = _calc_cv(s)
        range_ratio = round(float(s.max() / s.min()), 2) if s.min() > 0 else None
        iqr = float(s.quantile(0.75) - s.quantile(0.25))
        iqr_ratio = round(iqr / float(s.median()) * 100, 2) if s.median() > 0 else None

        flag = '🟢'
        if cv and cv > 20:
            flag = '🔴'
        elif range_ratio and range_ratio > 2.0:
            flag = '🔴'
        elif cv and cv > 15:
            flag = '🟡'

        results.append({
            'group': group_name,
            'count': len(s),
            'mean': round(float(s.mean()), 0),
            'median': round(float(s.median()), 0),
            'cv': cv,
            'range_ratio': range_ratio,
            'iqr_ratio': iqr_ratio,
            'flag': flag,
        })

    flagged = [r for r in results if r['flag'] in ('🔴', '🟡')]
    return {
        'total_groups': len(results),
        'flagged_count': len(flagged),
        'groups': sorted(results, key=lambda x: -(x.get('cv') or 0)),
    }

def _dim2_cross_position(df, salary_col, position_col, level_col):
    """Dimension 2: Cross-position equity at same level.

    Compares median salary across positions within same level.
    Flags positions deviating > 15% from level median.
    """
    if not all(c and c in df.columns for c in [salary_col, position_col, level_col]):
        return {'error': 'Missing required columns', 'comparisons': []}

    results = []
    for level, ldf in df.groupby(level_col):
        level_median = float(ldf[salary_col].median())
        if level_median == 0:
            continue
        for pos, pdf in ldf.groupby(position_col):
            if len(pdf) < 2:
                continue
            pos_median = float(pdf[salary_col].median())
            deviation = round((pos_median - level_median) / level_median * 100, 1)
            flag = '🟢'
            if abs(deviation) > 15:
                flag = '🟡'
            if abs(deviation) > 25:
                flag = '🔴'
            results.append({
                'level': str(level),
                'position': str(pos),
                'count': len(pdf),
                'median': round(pos_median, 0),
                'level_median': round(level_median, 0),
                'deviation_pct': deviation,
                'flag': flag,
            })

    flagged = [r for r in results if r['flag'] in ('🔴', '🟡')]
    return {
        'total_comparisons': len(results),
        'flagged_count': len(flagged),
        'comparisons': sorted(results, key=lambda x: -abs(x['deviation_pct'])),
    }

def _dim3_regression(df, salary_col, level_col, tenure_col=None):
    """Dimension 3: Salary-tenure regression analysis.

    Model: ln(salary) = β0 + β1·grade_numeric + β2·tenure + ε
    Flags individuals with residuals beyond ±1.65 SD.
    """
    if scipy_stats is None:
        return {'error': 'scipy not available', 'anomalies': []}
    if not salary_col or salary_col not in df.columns:
        return {'error': 'Missing salary column', 'anomalies': []}

    rdf = df[[salary_col]].copy()
    rdf['ln_salary'] = np.log(rdf[salary_col].clip(lower=1))

    # Encode level as numeric
    features = []
    if level_col and level_col in df.columns:
        level_order = sorted(df[level_col].dropna().unique(), key=str)
        level_map = {v: i for i, v in enumerate(level_order)}
        rdf['grade_num'] = df[level_col].map(level_map)
        features.append('grade_num')

    if tenure_col and tenure_col in df.columns:
        rdf['tenure'] = pd.to_numeric(df[tenure_col], errors='coerce')
        features.append('tenure')

    if not features:
        return {'error': 'No features (level or tenure) available for regression', 'anomalies': []}

    # Drop NaN rows
    valid = rdf.dropna(subset=['ln_salary'] + features)
    if len(valid) < 10:
        return {'error': f'Insufficient data for regression (n={len(valid)})', 'anomalies': []}

    # OLS regression
    X = valid[features].values
    X = np.column_stack([np.ones(len(X)), X])
    y = valid['ln_salary'].values

    try:
        coeffs, residuals_sum, rank, sv = np.linalg.lstsq(X, y, rcond=None)

        y_pred = X @ coeffs
        residuals = y - y_pred

        # R-squared
        ss_res = np.sum(residuals ** 2)
        ss_tot = np.sum((y - y.mean()) ** 2)
        r_squared = round(float(1 - ss_res / ss_tot), 4) if ss_tot > 0 else 0

        # Residual analysis
        res_std = float(np.std(residuals))
        threshold = 1.65

        anomalies = []
        for i, (idx, row) in enumerate(valid.iterrows()):
            z = residuals[i] / res_std if res_std > 0 else 0
            if abs(z) > threshold:
                direction = 'high' if z > 0 else 'low'
                anomalies.append({
                    'index': int(idx),
                    'salary': round(float(row[salary_col] if salary_col in row.index else np.exp(row['ln_salary'])), 0),
                    'predicted_ln': round(float(y_pred[i]), 4),
                    'residual_z': round(float(z), 2),
                    'direction': direction,
                    'flag': '🔴',
                })
    except Exception as e:
        return {'error': f'Regression failed: {e}', 'anomalies': []}

    # Beta coefficients with labels
    beta_labels = ['intercept'] + features
    betas = {label: round(float(c), 4) for label, c in zip(beta_labels, coeffs)}

    return {
        'r_squared': r_squared,
        'betas': betas,
        'residual_std': round(res_std, 4),
        'n_samples': len(valid),
        'anomaly_count': len(anomalies),
        'anomalies': anomalies,
        'model_note': 'R² < 0.3: level explains limited salary variance' if r_squared < 0.3 else '',
    }

def _dim4_inversion(df, salary_col, hire_col, group_cols):
    """Dimension 4: Salary inversion detection.

    Within each position x level group, compares median salary of
    new employees (tenure < 2yr) vs veteran employees (tenure > 5yr).
    Inversion = new median > veteran median.
    """
    if not salary_col or not hire_col:
        return {'error': 'Missing salary or hire_date column', 'inversions': []}

    if salary_col not in df.columns or hire_col not in df.columns:
        return {'error': 'Salary or hire_date column not in DataFrame', 'inversions': []}

    valid_cols = [c for c in group_cols if c and c in df.columns]
    if not valid_cols:
        return {'error': 'No valid group columns', 'inversions': []}

    rdf = df.copy()
    rdf['_tenure_yr'] = _tenure_years(rdf, hire_col)
    rdf['_group'] = rdf[valid_cols].astype(str).agg(' × '.join, axis=1)

    inversions = []
    for group_name, gdf in rdf.groupby('_group'):
        new_emp = gdf[gdf['_tenure_yr'] < 2][salary_col].dropna()
        vet_emp = gdf[gdf['_tenure_yr'] > 5][salary_col].dropna()

        if len(new_emp) < 3 or len(vet_emp) < 3:
            inversions.append({
                'group': group_name,
                'new_count': len(new_emp),
                'vet_count': len(vet_emp),
                'note': 'sample_insufficient',
                'flag': '⚪',
            })
            continue

        new_median = float(new_emp.median())
        vet_median = float(vet_emp.median())

        if vet_median > 0 and new_median > vet_median:
            gap_pct = round((new_median - vet_median) / vet_median * 100, 1)
            inversions.append({
                'group': group_name,
                'new_count': len(new_emp),
                'new_median': round(new_median, 0),
                'vet_count': len(vet_emp),
                'vet_median': round(vet_median, 0),
                'gap_pct': gap_pct,
                'flag': '🔴',
            })
        else:
            inversions.append({
                'group': group_name,
                'new_count': len(new_emp),
                'new_median': round(new_median, 0),
                'vet_count': len(vet_emp),
                'vet_median': round(vet_median, 0),
                'gap_pct': round((new_median - vet_median) / vet_median * 100, 1) if vet_median > 0 else 0,
                'flag': '🟢',
            })

    inverted = [i for i in inversions if i['flag'] == '🔴']
    return {
        'total_groups': len(inversions),
        'inverted_count': len(inverted),
        'inversions': inversions,
    }

def _dim5_structure_fit(df, col_map):
    """Dimension 5: Salary structure fitness.

    Checks if fixed/variable ratio matches job nature:
    - Management/Professional: fixed >= 70%
    - Sales: variable 40-60%
    - Operations: fixed >= 80%
    """
    det = col_map.get('detected', {})
    sal_comp = col_map.get('salary_components', {})
    position_col = det.get('position')
    base_col = det.get('base_salary')

    if not position_col or position_col not in df.columns:
        return {'error': 'Missing position column', 'assessments': []}

    # Build fixed and variable totals
    fixed_cols = [c for c in [base_col, sal_comp.get('position_allowance'), sal_comp.get('allowance')]
                  if c and c in df.columns]
    var_cols = [c for c in [sal_comp.get('performance'), sal_comp.get('commission'),
                            sal_comp.get('overtime'), sal_comp.get('bonus')]
                if c and c in df.columns]

    if not fixed_cols and not var_cols:
        return {'error': 'No salary component columns found', 'assessments': []}

    rdf = df.copy()
    rdf['_fixed'] = rdf[fixed_cols].fillna(0).sum(axis=1) if fixed_cols else 0
    rdf['_variable'] = rdf[var_cols].fillna(0).sum(axis=1) if var_cols else 0
    rdf['_total'] = rdf['_fixed'] + rdf['_variable']
    rdf['_fixed_ratio'] = (rdf['_fixed'] / rdf['_total'].replace(0, np.nan) * 100)

    # Classify positions (heuristic)
    def _classify_job(pos_name):
        pos = str(pos_name)
        if any(kw in pos for kw in ['销售', '业务', 'sales', 'BD', '客户经理']):
            return 'sales'
        if any(kw in pos for kw in ['操作', '生产', '制造', '装配', '仓库', '物流', '司机']):
            return 'operations'
        return 'professional'

    assessments = []
    for pos, pdf in rdf.groupby(position_col):
        if len(pdf) < 2:
            continue
        job_type = _classify_job(pos)
        avg_fixed_ratio = round(float(pdf['_fixed_ratio'].mean()), 1)

        flag = '🟢'
        note = ''
        if job_type == 'sales':
            var_ratio = 100 - avg_fixed_ratio
            if var_ratio < 30:
                flag = '🟡'
                note = f'Sales role but variable only {var_ratio:.0f}% (expected 40-60%)'
        elif job_type == 'operations':
            if avg_fixed_ratio < 80:
                flag = '🟡'
                note = f'Operations role but fixed only {avg_fixed_ratio:.0f}% (expected >=80%)'
        else:
            if avg_fixed_ratio < 70:
                flag = '🟡'
                note = f'Professional role but fixed only {avg_fixed_ratio:.0f}% (expected >=70%)'

        assessments.append({
            'position': str(pos),
            'job_type': job_type,
            'count': len(pdf),
            'avg_fixed_ratio': avg_fixed_ratio,
            'flag': flag,
            'note': note,
        })

    flagged = [a for a in assessments if a['flag'] != '🟢']
    return {
        'total_positions': len(assessments),
        'flagged_count': len(flagged),
        'assessments': assessments,
    }

def _dim6_compa_ratio(df, salary_col, group_cols):
    """Dimension 6: Internal Compa-Ratio analysis.

    CR = employee salary / group median * 100%
    Bands: <80% 🔴, 80-90% 🟡, 90-110% 🟢, 110-120% 🟡, >120% 🔴
    """
    if not salary_col or salary_col not in df.columns:
        return {'error': 'Missing salary column', 'distribution': {}}

    valid_cols = [c for c in group_cols if c and c in df.columns]
    if not valid_cols:
        # Use entire dataset as one group
        valid_cols = []

    rdf = df.copy()

    if valid_cols:
        rdf['_group'] = rdf[valid_cols].astype(str).agg(' × '.join, axis=1)
        group_medians = rdf.groupby('_group')[salary_col].transform('median')
    else:
        group_medians = rdf[salary_col].median()

    rdf['_cr'] = round(rdf[salary_col] / pd.Series(group_medians, index=rdf.index).replace(0, np.nan) * 100, 1)

    # Distribution
    def _cr_band(cr):
        if pd.isna(cr):
            return 'unknown'
        if cr < 80:
            return 'very_low'
        if cr < 90:
            return 'low'
        if cr <= 110:
            return 'normal'
        if cr <= 120:
            return 'high'
        return 'very_high'

    rdf['_cr_band'] = rdf['_cr'].apply(_cr_band)

    distribution = rdf['_cr_band'].value_counts().to_dict()
    total = len(rdf)
    distribution_pct = {k: round(v / total * 100, 1) for k, v in distribution.items()}

    # Flagged individuals
    flagged_low = rdf[rdf['_cr'] < 80].index.tolist()
    flagged_high = rdf[rdf['_cr'] > 120].index.tolist()

    # CR stats
    cr_series = rdf['_cr'].dropna()

    return {
        'mean_cr': round(float(cr_series.mean()), 1) if len(cr_series) > 0 else None,
        'median_cr': round(float(cr_series.median()), 1) if len(cr_series) > 0 else None,
        'std_cr': round(float(cr_series.std()), 1) if len(cr_series) > 0 else None,
        'distribution': distribution,
        'distribution_pct': distribution_pct,
        'compliance_rate': distribution_pct.get('normal', 0),
        'flagged_low_count': len(flagged_low),
        'flagged_high_count': len(flagged_high),
        'flagged_low_indices': flagged_low,
        'flagged_high_indices': flagged_high,
        'cr_column': '_cr',
    }


def _step4_diagnose(df, col_map):
    """Step 4: Full six-dimension fairness diagnosis.

    Args:
        df: Cleaned DataFrame (from step1)
        col_map: Column mapping (from _detect_columns or step1 results)

    Returns:
        dict with health_metrics, dim1-dim6 results, anomaly_list, root_causes
    """
    det = col_map.get('detected', {}) if isinstance(col_map, dict) else col_map
    sal_comp = col_map.get('salary_components', {}) if isinstance(col_map, dict) else {}

    salary_col = det.get('base_salary') or det.get('gross')
    level_col = det.get('level')
    position_col = det.get('position')
    hire_col = det.get('hire_date')
    dept_col = det.get('department')

    if not salary_col or salary_col not in df.columns:
        return {'error': 'No salary column found in data'}

    # Prepare group columns for various dimensions
    group_cols = [c for c in [position_col, level_col] if c and c in df.columns]

    # Compute tenure if possible
    tenure_col = None
    if hire_col and hire_col in df.columns:
        df = df.copy()
        df['_tenure_yr'] = _tenure_years(df, hire_col)
        tenure_col = '_tenure_yr'

    # Health metrics
    gini = _calc_gini(df[salary_col])

    # Run six dimensions
    dim1 = _dim1_internal_equity(df, salary_col, group_cols)
    dim2 = _dim2_cross_position(df, salary_col, position_col, level_col) if position_col and level_col else {'error': 'Missing position or level column'}
    dim3 = _dim3_regression(df, salary_col, level_col, tenure_col)
    dim4 = _dim4_inversion(df, salary_col, hire_col, group_cols) if hire_col else {'error': 'Missing hire_date column'}
    dim5 = _dim5_structure_fit(df, col_map)
    dim6 = _dim6_compa_ratio(df, salary_col, group_cols)

    # Aggregate health metrics
    health = {
        'gini': gini,
        'salary_level_r2': dim3.get('r_squared'),
        'cr_compliance_rate': dim6.get('compliance_rate'),
        'inversion_rate': round(dim4.get('inverted_count', 0) / max(dim4.get('total_groups', 1), 1) * 100, 1) if isinstance(dim4, dict) and 'inverted_count' in dim4 else None,
        'anomaly_count_regression': dim3.get('anomaly_count', 0),
        'flagged_internal': dim1.get('flagged_count', 0),
    }

    # Collect all red-flagged items
    anomaly_list = []
    # From dim3 regression anomalies
    for a in dim3.get('anomalies', []):
        anomaly_list.append({
            'source': 'regression',
            'index': a['index'],
            'salary': a['salary'],
            'detail': f"Residual z={a['residual_z']} ({a['direction']})",
        })
    # From dim6 CR anomalies
    for idx in dim6.get('flagged_low_indices', []):
        if idx in df.index:
            anomaly_list.append({
                'source': 'compa_ratio_low',
                'index': int(idx),
                'salary': round(float(df.loc[idx, salary_col]), 0) if salary_col in df.columns else None,
                'detail': 'CR < 80%',
            })
    for idx in dim6.get('flagged_high_indices', []):
        if idx in df.index:
            anomaly_list.append({
                'source': 'compa_ratio_high',
                'index': int(idx),
                'salary': round(float(df.loc[idx, salary_col]), 0) if salary_col in df.columns else None,
                'detail': 'CR > 120%',
            })

    # Root cause statistics
    root_causes = {}
    for a in anomaly_list:
        src = a['source']
        root_causes[src] = root_causes.get(src, 0) + 1

    results = {
        'health_metrics': health,
        'dim1_internal': dim1,
        'dim2_cross': dim2,
        'dim3_regression': dim3,
        'dim4_inversion': dim4,
        'dim5_structure': dim5,
        'dim6_compa': dim6,
        'anomaly_list': anomaly_list,
        'anomaly_count': len(anomaly_list),
        'root_causes': root_causes,
    }

    # Cache (strip large index lists for JSON)
    cache_data = _json.loads(_json.dumps(results, default=str))
    _cache_result('step4', cache_data)

    # Display results
    print("=== Health Metrics ===")
    for k, v in health.items():
        print(f"  {k}: {v}")
    print("\n=== Six-Dimension Diagnosis ===")
    print(f"  Internal equity: {dim1.get('flagged_count',0)}/{dim1.get('total_groups',0)} groups flagged")
    if isinstance(dim2, dict) and 'flagged_count' in dim2:
        print(f"  Cross-position: {dim2['flagged_count']} deviations")
    print(f"  Regression: R2={dim3.get('r_squared','N/A')}, {dim3.get('anomaly_count',0)} anomalies")
    if isinstance(dim4, dict) and 'inverted_count' in dim4:
        print(f"  Inversion: {dim4['inverted_count']} groups")
    print(f"  Structure: {dim5.get('flagged_count',0)} mismatches")
    print(f"  CR: compliance={dim6.get('compliance_rate','N/A')}%, low={dim6.get('flagged_low_count',0)}, high={dim6.get('flagged_high_count',0)}")
    print(f"\n=== Anomaly Summary: {len(anomaly_list)} total ===")
    for src, cnt in root_causes.items():
        print(f"  {src}: {cnt}")

    # Auto-validate
    try:
        val = _validate_step4(df, col_map, results)
        results['validation'] = val
        if val.get('corrections'):
            for c in val['corrections']:
                if c['metric'] == 'gini':
                    results['health_metrics']['gini'] = c['new']
            _cache_result('step4', _json.loads(_json.dumps(results, default=str)))
    except Exception as e:
        print(f"[validate_step4] Warning: {e}")

    return results

def _validate_step4(df, col_map, diagnosis):
    """Built-in validation for Step 4 diagnosis results."""
    det = col_map.get('detected', {}) if isinstance(col_map, dict) else col_map
    salary_col = det.get('base_salary') or det.get('gross')
    level_col = det.get('level')
    position_col = det.get('position')

    issues = []
    corrections = []
    lines = ['=== Step 4 Validation ===']

    # 1. Re-compute Gini
    if salary_col and salary_col in df.columns:
        recomputed_gini = _calc_gini(df[salary_col])
        reported_gini = diagnosis.get('health_metrics', {}).get('gini')
        lines.append(f'\n1. Gini: reported={reported_gini}, recomputed={recomputed_gini}')
        if reported_gini is not None and recomputed_gini is not None:
            if abs(reported_gini - recomputed_gini) > 0.01:
                issues.append(f'Gini mismatch: {reported_gini} vs {recomputed_gini}')
                corrections.append({'metric': 'gini', 'old': reported_gini, 'new': recomputed_gini})

    # 2. Re-compute CR compliance
    group_cols = [c for c in [position_col, level_col] if c and c in df.columns]
    if salary_col and salary_col in df.columns:
        rdf = df.copy()
        if group_cols:
            rdf['_group'] = rdf[group_cols].astype(str).agg(' x '.join, axis=1)
            group_medians = rdf.groupby('_group')[salary_col].transform('median')
        else:
            group_medians = rdf[salary_col].median()
        cr = rdf[salary_col] / pd.Series(group_medians, index=rdf.index).replace(0, np.nan) * 100
        recomputed_compliance = round(float(((cr >= 90) & (cr <= 110)).mean() * 100), 1)
        reported_compliance = diagnosis.get('dim6_compa', {}).get('compliance_rate')
        lines.append(f'   CR compliance: reported={reported_compliance}%, recomputed={recomputed_compliance}%')
        if reported_compliance is not None and abs(reported_compliance - recomputed_compliance) > 1.0:
            issues.append(f'CR compliance mismatch: {reported_compliance}% vs {recomputed_compliance}%')

    # 3. Spot-check red-flags
    anomaly_list = diagnosis.get('anomaly_list', [])
    red_flags = [a for a in anomaly_list if a.get('source') in ('compa_ratio_low', 'regression')]
    checked = 0
    mismatches = 0
    for a in red_flags[:5]:
        idx = a.get('index')
        if idx is None or idx not in df.index:
            continue
        checked += 1
        if a.get('source') == 'compa_ratio_low' and salary_col in df.columns:
            actual_cr = float(cr.loc[idx]) if idx in cr.index else None
            if actual_cr is not None and actual_cr >= 80:
                mismatches += 1
                issues.append(f'False red-flag at index {idx}: CR={actual_cr:.1f}% >= 80%')
    lines.append(f'\n3. Spot-check: {checked} checked, {mismatches} mismatches')
    if mismatches > 0:
        issues.append(f'{mismatches}/{checked} spot-checks failed')

    # 4. Cross-dimension consistency
    cr_low_indices = set(diagnosis.get('dim6_compa', {}).get('flagged_low_indices', []))
    regression_low = set(
        a['index'] for a in diagnosis.get('dim3_regression', {}).get('anomalies', [])
        if a.get('direction') == 'low'
    )
    overlap = cr_low_indices & regression_low
    lines.append(f'\n4. Cross-dim: CR<80%={len(cr_low_indices)}, reg-low={len(regression_low)}, overlap={len(overlap)}')

    # 5. Inversion sample size
    inversions = diagnosis.get('dim4_inversion', {}).get('inversions', [])
    insufficient = [i for i in inversions if i.get('note') == 'sample_insufficient']
    lines.append(f'\n5. Inversion: {len(insufficient)} groups with insufficient samples')

    passed = len(issues) == 0
    lines.append(f'\n=== {"PASS" if passed else f"FAIL ({len(issues)} issues)"} ===')
    for iss in issues:
        lines.append(f'  ! {iss}')
    display = '\n'.join(lines)
    print(display)
    return {'passed': passed, 'issues': issues, 'corrections': corrections, 'display': display}

# ============================================================
# 1e. Step 5: Compensation Adjustment Scenarios
# ============================================================

def _step5_scenarios(df, col_map, diagnosis=None):
    """Step 5: Three-tier adjustment scenarios + ROI.

    Args:
        df: Cleaned DataFrame
        col_map: Column mapping
        diagnosis: Step 4 results (loaded from cache if None)

    Returns:
        dict with scenarios A/B/C, roi analysis
    """
    det = col_map.get('detected', {}) if isinstance(col_map, dict) else col_map

    salary_col = det.get('base_salary') or det.get('gross')
    if not salary_col or salary_col not in df.columns:
        return {'error': 'No salary column found'}

    # Load diagnosis if not provided
    if diagnosis is None:
        diagnosis = _load_cached('step4')
    if not diagnosis:
        return {'error': 'Step 4 diagnosis results not found. Run _step4_diagnose first.'}

    # Get group columns and compute CR
    level_col = det.get('level')
    position_col = det.get('position')
    group_cols = [c for c in [position_col, level_col] if c and c in df.columns]

    rdf = df.copy()
    if group_cols:
        rdf['_group'] = rdf[group_cols].astype(str).agg(' × '.join, axis=1)
        group_medians = rdf.groupby('_group')[salary_col].transform('median')
    else:
        group_medians = rdf[salary_col].median()
    rdf['_cr'] = rdf[salary_col] / pd.Series(group_medians, index=rdf.index).replace(0, np.nan) * 100

    # Group-level percentiles for targets
    if group_cols:
        p25 = rdf.groupby('_group')[salary_col].transform(lambda x: x.quantile(0.25))
        p40 = rdf.groupby('_group')[salary_col].transform(lambda x: x.quantile(0.40))
    else:
        p25 = rdf[salary_col].quantile(0.25)
        p40 = rdf[salary_col].quantile(0.40)

    # Regression anomaly indices from dim3
    regression_anomaly_indices = set()
    dim3 = diagnosis.get('dim3_regression', {})
    for a in dim3.get('anomalies', []):
        if a.get('direction') == 'low':
            regression_anomaly_indices.add(a['index'])

    def _calc_scenario(name, criteria_fn, target_fn, desc):
        """Calculate a single adjustment scenario."""
        eligible = rdf[criteria_fn(rdf)].copy()
        if len(eligible) == 0:
            return {
                'name': name, 'description': desc,
                'count': 0, 'annual_budget': 0,
                'avg_increase_pct': 0, 'details': [],
            }

        eligible['_target'] = target_fn(eligible)
        eligible['_increase'] = (eligible['_target'] - eligible[salary_col]).clip(lower=0)
        eligible['_increase_pct'] = round(eligible['_increase'] / eligible[salary_col].replace(0, np.nan) * 100, 1)

        # Only include those needing actual increase
        actual = eligible[eligible['_increase'] > 0]

        annual_budget = float(actual['_increase'].sum() * 12)
        avg_pct = round(float(actual['_increase_pct'].mean()), 1) if len(actual) > 0 else 0

        # Simulate post-adjustment CR
        post_df = rdf.copy()
        post_df.loc[actual.index, salary_col] = actual['_target']
        if group_cols:
            new_medians = post_df.groupby('_group')[salary_col].transform('median')
        else:
            new_medians = post_df[salary_col].median()
        post_cr = post_df[salary_col] / pd.Series(new_medians, index=post_df.index).replace(0, np.nan) * 100
        new_compliance = round(float(((post_cr >= 90) & (post_cr <= 110)).mean() * 100), 1)

        return {
            'name': name,
            'description': desc,
            'count': len(actual),
            'annual_budget': round(annual_budget, 0),
            'monthly_budget': round(float(actual['_increase'].sum()), 0),
            'avg_increase_pct': avg_pct,
            'post_cr_compliance': new_compliance,
        }

    # Scenario A: Fix severe only (CR < 80% AND regression anomaly)
    def crit_a(r):
        return (r['_cr'] < 80) & (r.index.isin(regression_anomaly_indices))
    def target_a(r):
        return pd.Series(p25, index=r.index) if isinstance(p25, (int, float)) else p25.loc[r.index]

    # Scenario B: Fix severe + moderate (CR < 80% → P25, CR 80-90% → P40)
    def crit_b(r):
        return r['_cr'] < 90
    def target_b(r):
        result = pd.Series(index=r.index, dtype=float)
        mask_severe = r['_cr'] < 80
        mask_moderate = (r['_cr'] >= 80) & (r['_cr'] < 90)
        if isinstance(p25, (int, float)):
            result[mask_severe] = p25
            result[mask_moderate] = p40
        else:
            result[mask_severe] = p25.loc[r.index[mask_severe]]
            result[mask_moderate] = p40.loc[r.index[mask_moderate]]
        return result

    # Scenario C: Full alignment (all CR → 90%+)
    def crit_c(r):
        return r['_cr'] < 90
    def target_c(r):
        # Target: group_median * 0.9
        if group_cols:
            medians = rdf.groupby('_group')[salary_col].transform('median')
            return (medians.loc[r.index] * 0.9).clip(lower=r[salary_col])
        else:
            med = rdf[salary_col].median()
            return pd.Series(max(med * 0.9, 0), index=r.index).clip(lower=r[salary_col])

    scenario_a = _calc_scenario('A', crit_a, target_a, 'Fix severe only: CR<80% + regression anomaly → P25')
    scenario_b = _calc_scenario('B', crit_b, target_b, 'Fix severe + moderate: CR<80%→P25, CR 80-90%→P40')
    scenario_c = _calc_scenario('C', crit_c, target_c, 'Full alignment: all CR → 90%+')

    # ROI calculation
    # Assumption: replacement cost = 1.5x annual salary for high-risk employees
    at_risk = rdf[rdf['_cr'] < 80]
    avg_salary = float(at_risk[salary_col].mean()) if len(at_risk) > 0 else 0
    turnover_risk_count = len(at_risk)
    replacement_cost_per = avg_salary * 12 * 1.5  # 1.5x annual salary
    potential_loss = replacement_cost_per * turnover_risk_count * 0.3  # 30% turnover probability

    investment_b = scenario_b['annual_budget']
    roi = {
        'at_risk_count': turnover_risk_count,
        'avg_salary': round(avg_salary, 0),
        'replacement_cost_assumption': '1.5x annual salary',
        'potential_annual_loss': round(potential_loss, 0),
        'investment_scenario_b': round(investment_b, 0),
        'roi_1yr_pct': round((potential_loss - investment_b) / max(investment_b, 1) * 100, 1) if investment_b > 0 else 0,
        'roi_2yr_pct': round((potential_loss * 2 - investment_b) / max(investment_b, 1) * 100, 1) if investment_b > 0 else 0,
    }

    results = {
        'scenarios': {
            'A': scenario_a,
            'B': scenario_b,
            'C': scenario_c,
        },
        'roi': roi,
        'current_cr_compliance': diagnosis.get('dim6_compa', {}).get('compliance_rate', 0),
    }

    _cache_result('step5', results)

    # Display results
    print("=== Three-Tier Adjustment Scenarios ===")
    for key in ['A', 'B', 'C']:
        s = results['scenarios'][key]
        print(f"\n  Scenario {key}: {s['description']}")
        print(f"    Coverage: {s['count']} people")
        print(f"    Annual budget: {s['annual_budget']:,.0f}")
        print(f"    Avg increase: {s['avg_increase_pct']}%")
        print(f"    Post-CR compliance: {s.get('post_cr_compliance', 'N/A')}%")
    print(f"\n=== ROI Analysis ===")
    print(f"  At-risk: {roi['at_risk_count']}")
    print(f"  Potential loss: {roi['potential_annual_loss']:,.0f}")
    print(f"  Investment (B): {roi['investment_scenario_b']:,.0f}")
    print(f"  1yr ROI: {roi['roi_1yr_pct']}%")

    return results

# ============================================================
# 1f. Step 5: Build Report Sections
# ============================================================

def _step5_build_report_sections(df, col_map, diagnosis=None, scenarios=None):
    """Build complete report sections JSON from cached analysis results.

    Args:
        df: Cleaned DataFrame (_df)
        col_map: Column mapping (from step1)
        diagnosis: Step 4 diagnosis results (auto-loads from cache if None)
        scenarios: Step 5 scenarios results (auto-loads from cache if None)

    Returns:
        dict with:
            'sections': list of section dicts (9 sections)
            'file_path': relative path to written JSON file
    """
    # Auto-load dependencies
    if diagnosis is None:
        diagnosis = _load_cached('step4')
    if not diagnosis:
        return {'error': 'Step 4 results not found. Run _step4_diagnose first.'}
    if scenarios is None:
        scenarios = _load_cached('step5')
    if not scenarios:
        scenarios = _step5_scenarios(df, col_map, diagnosis)

    step1 = _load_cached('step1')

    det = col_map.get('detected', {}) if isinstance(col_map, dict) else col_map
    sal_comp = col_map.get('salary_components', {}) if isinstance(col_map, dict) else {}
    salary_col = det.get('base_salary') or det.get('gross')
    level_col = det.get('level')
    position_col = det.get('position')
    id_col = det.get('id')
    name_col = det.get('name')
    dept_col = det.get('department')

    health = diagnosis.get('health_metrics', {})
    anomaly_list = diagnosis.get('anomaly_list', [])
    anomaly_count = diagnosis.get('anomaly_count', len(anomaly_list))

    # Helper: format number with thousands separator
    def _fmt(v, decimals=0):
        if v is None or (isinstance(v, float) and _math.isnan(v)):
            return 'N/A'
        if decimals == 0:
            return f'{v:,.0f}'
        return f'{v:,.{decimals}f}'

    def _pct(v):
        if v is None:
            return 'N/A'
        return f'{v:.1f}%'

    def _state_gini(v):
        if v is None: return 'neutral'
        return 'warn' if v > 0.3 else 'good'

    def _state_cr(v):
        if v is None: return 'neutral'
        return 'good' if v > 70 else 'warn'

    def _state_inv(v):
        if v is None: return 'neutral'
        return 'warn' if v > 10 else 'good'

    sections = []

    # ── Section 1: 管理层摘要 ──
    gini_val = health.get('gini')
    cr_rate = health.get('cr_compliance_rate')
    inv_rate = health.get('inversion_rate')
    r2_val = health.get('salary_level_r2')

    highlight_parts = [f'本次分析覆盖 {len(df)} 名员工']
    if anomaly_count > 0:
        highlight_parts.append(f'发现 {anomaly_count} 例薪酬异常')
    if cr_rate is not None:
        highlight_parts.append(f'CR合规率 {_pct(cr_rate)}')
    highlight_text = '，'.join(highlight_parts) + '。'

    summary_metrics = [
        {'label': 'Gini 系数', 'value': str(gini_val) if gini_val is not None else 'N/A', 'state': _state_gini(gini_val)},
        {'label': 'CR 合规率', 'value': _pct(cr_rate), 'state': _state_cr(cr_rate)},
        {'label': '倒挂率', 'value': _pct(inv_rate), 'state': _state_inv(inv_rate)},
        {'label': '高风险人数', 'value': str(anomaly_count), 'state': 'bad' if anomaly_count > 10 else 'neutral'},
    ]
    if r2_val is not None:
        summary_metrics.append({
            'label': '职级-薪酬 R²',
            'value': str(r2_val),
            'state': 'warn' if r2_val < 0.3 else 'good',
        })

    # Build content with key findings
    findings = []
    if gini_val is not None and gini_val > 0.3:
        findings.append(f'- Gini 系数 {gini_val} 偏高，薪酬分配均匀度不足')
    if cr_rate is not None and cr_rate < 70:
        findings.append(f'- CR 合规率仅 {_pct(cr_rate)}，大量员工薪酬偏离市场中位')
    if inv_rate is not None and inv_rate > 10:
        findings.append(f'- 倒挂率 {_pct(inv_rate)}，新老员工薪酬存在显著倒挂')
    if anomaly_count > 10:
        findings.append(f'- 发现 {anomaly_count} 例高风险异常，需优先关注')
    if r2_val is not None and r2_val < 0.3:
        findings.append(f'- 职级对薪酬解释力不足（R²={r2_val}），薪酬体系规范性待加强')

    summary_content = '**核心发现：**\n' + '\n'.join(findings) if findings else '整体薪酬公平性处于合理范围。'
    summary_content += '\n\n**不行动的代价：** 薪酬不公平将导致核心人才流失、团队士气下降，问题恶化后补救成本将远超当前调整投入。'

    sections.append({
        'heading': '管理层摘要',
        'highlight': highlight_text,
        'metrics': summary_metrics,
        'content': summary_content,
    })

    # ── Section 2: 数据概览 ──
    total_retained = step1.get('total_retained', len(df)) if step1 else len(df)
    total_excluded = step1.get('total_excluded', 0) if step1 else 0

    # Count unique positions and levels
    n_positions = int(df[position_col].nunique()) if position_col and position_col in df.columns else 0
    n_levels = int(df[level_col].nunique()) if level_col and level_col in df.columns else 0

    data_metrics = [
        {'label': '分析人数', 'value': str(total_retained), 'state': 'neutral'},
        {'label': '岗位族数', 'value': str(n_positions), 'state': 'neutral'},
    ]
    if n_levels > 0:
        data_metrics.append({'label': '职级数', 'value': str(n_levels), 'state': 'neutral'})

    data_content = f'本次分析基于 {total_retained} 名在职员工薪酬数据'
    if total_excluded > 0:
        data_content += f'，已排除 {total_excluded} 条不适用记录（离职、非全职、零薪资等）'
    data_content += '。'
    excl_summary = step1.get('exclusion_summary', {}) if step1 else {}
    if excl_summary:
        reasons = []
        for reason, count in excl_summary.items():
            if count > 0:
                reasons.append(f'{reason}: {count}人')
        if reasons:
            data_content += '\n\n**排除明细：** ' + '、'.join(reasons)

    # Position distribution table
    data_table = None
    if position_col and position_col in df.columns:
        pos_counts = df[position_col].value_counts()
        pos_rows = [[str(pos), str(cnt), _pct(cnt / len(df) * 100)] for pos, cnt in pos_counts.head(15).items()]
        if pos_rows:
            data_table = {
                'title': '岗位族人数分布',
                'columns': ['岗位族', '人数', '占比'],
                'rows': pos_rows,
            }

    sec2 = {
        'heading': '数据概览',
        'content': data_content,
        'metrics': data_metrics,
    }
    if data_table:
        sec2['table'] = data_table
    sections.append(sec2)

    # ── Section 3: 岗位体系与职级框架 ──
    sec3_content = '基于岗位名称自动归一化，构建岗位族分组。'
    if level_col and level_col in df.columns and position_col and position_col in df.columns:
        sec3_content += '以下为岗位族与职级的交叉矩阵，展示各组合的人数分布。'
        # Build cross-tab
        try:
            ct = pd.crosstab(df[position_col], df[level_col], margins=True, margins_name='合计')
            levels_sorted = sorted([c for c in ct.columns if c != '合计'], key=str)
            cols_order = levels_sorted + ['合计']
            ct = ct[[c for c in cols_order if c in ct.columns]]
            ct_cols = ['岗位族'] + [str(c) for c in ct.columns]
            ct_rows = []
            for idx_name, row in ct.iterrows():
                ct_rows.append([str(idx_name)] + [str(int(v)) for v in row.values])
            sec3_table = {
                'title': '岗位族 × 职级人数矩阵',
                'columns': ct_cols,
                'rows': ct_rows[-16:],  # limit rows
            }
        except Exception:
            sec3_table = None
    else:
        sec3_table = None

    sec3 = {'heading': '岗位体系与职级框架', 'content': sec3_content}
    if sec3_table:
        sec3['table'] = sec3_table
    sections.append(sec3)

    # ── Section 4: 六维度公平性诊断 ──
    dim_labels = [
        ('dim1_internal', '内部公平性', 'CV/极差比'),
        ('dim2_cross', '跨岗位公平性', '偏离度'),
        ('dim3_regression', '回归分析', 'R²'),
        ('dim4_inversion', '新老倒挂', '倒挂率'),
        ('dim5_structure', '结构匹配度', '固浮比'),
        ('dim6_compa', 'CR分布', '合规率'),
    ]

    dim_metrics = []
    dim_table_rows = []
    dim_content_parts = []

    for dim_key, dim_name, indicator in dim_labels:
        dim_data = diagnosis.get(dim_key, {})
        if isinstance(dim_data, dict) and 'error' in dim_data:
            val_str = '数据不足'
            state = 'neutral'
            detail = dim_data.get('error', '')
        elif dim_key == 'dim1_internal':
            flagged = dim_data.get('flagged_count', 0)
            total = dim_data.get('total_groups', 0)
            val_str = f'{flagged}/{total} 组异常'
            state = 'warn' if flagged > 0 else 'good'
            detail = f'共 {total} 个岗位×职级组，{flagged} 个组CV>15%或极差比>2.0'
        elif dim_key == 'dim2_cross':
            flagged = dim_data.get('flagged_count', 0)
            total = dim_data.get('total_comparisons', 0)
            val_str = f'{flagged}/{total} 对偏离'
            state = 'warn' if flagged > 0 else 'good'
            detail = f'{flagged} 对同职级跨岗位薪酬偏离>15%'
        elif dim_key == 'dim3_regression':
            r2 = dim_data.get('r_squared')
            n_anom = dim_data.get('anomaly_count', 0)
            val_str = f'R²={r2}' if r2 is not None else 'N/A'
            state = 'warn' if r2 is not None and r2 < 0.3 else 'good'
            detail = f'回归 R²={r2}，{n_anom} 例异常偏离'
        elif dim_key == 'dim4_inversion':
            inv_count = dim_data.get('inverted_count', 0)
            total = dim_data.get('total_groups', 0)
            inv_pct = round(inv_count / max(total, 1) * 100, 1)
            val_str = f'{inv_count}/{total} 组倒挂'
            state = 'warn' if inv_pct > 10 else 'good'
            detail = f'{inv_count} 个组存在新员工薪酬高于老员工'
        elif dim_key == 'dim5_structure':
            flagged = dim_data.get('flagged_count', 0)
            total = dim_data.get('total_positions', 0)
            val_str = f'{flagged}/{total} 岗不匹配'
            state = 'warn' if flagged > 0 else 'good'
            detail = f'{flagged} 个岗位固浮比与岗位性质不匹配'
        elif dim_key == 'dim6_compa':
            compliance = dim_data.get('compliance_rate', 0)
            val_str = _pct(compliance)
            state = 'good' if compliance > 70 else 'warn'
            low_cnt = dim_data.get('flagged_low_count', 0)
            high_cnt = dim_data.get('flagged_high_count', 0)
            detail = f'CR 90-110% 合规率 {_pct(compliance)}，低于80% {low_cnt}人，高于120% {high_cnt}人'
        else:
            val_str = 'N/A'
            state = 'neutral'
            detail = ''

        dim_metrics.append({'label': dim_name, 'value': val_str, 'state': state})
        dim_table_rows.append([dim_name, indicator, val_str, '正常' if state == 'good' else ('警告' if state == 'warn' else '—')])
        if detail:
            dim_content_parts.append(f'**{dim_name}**：{detail}')

    sec4_content = '\n\n'.join(dim_content_parts) if dim_content_parts else '六维度诊断完成。'

    sections.append({
        'heading': '六维度公平性诊断',
        'content': sec4_content,
        'metrics': dim_metrics,
        'table': {
            'title': '维度诊断汇总',
            'columns': ['维度', '指标', '结果', '评价'],
            'rows': dim_table_rows,
        },
    })

    # ── Section 5: 高优先级异常清单 ──
    # Use employee ID (if available) instead of name for PII protection
    label_col = id_col or name_col  # prefer ID over name
    label_col_name = '工号' if id_col else ('姓名' if name_col else '行号')

    anomaly_rows = []
    for a in anomaly_list[:20]:  # limit to 20 for report length
        idx = a.get('index')
        emp_label = ''
        emp_level = ''
        emp_salary = _fmt(a.get('salary'))
        emp_cr = ''
        emp_dept = ''

        if idx is not None and idx in df.index:
            row = df.loc[idx]
            if label_col and label_col in df.columns:
                emp_label = str(row.get(label_col, ''))
            else:
                emp_label = str(idx)
            if level_col and level_col in df.columns:
                emp_level = str(row.get(level_col, ''))
            if dept_col and dept_col in df.columns:
                emp_dept = str(row.get(dept_col, ''))
        else:
            emp_label = str(idx) if idx is not None else ''

        source_map = {
            'regression': '回归异常',
            'compa_ratio_low': 'CR偏低(<80%)',
            'compa_ratio_high': 'CR偏高(>120%)',
        }
        source_label = source_map.get(a.get('source', ''), a.get('source', ''))

        anomaly_rows.append([
            emp_label, emp_dept, emp_level, emp_salary, source_label, a.get('detail', ''),
        ])

    sec5_content = f'共发现 {anomaly_count} 例薪酬异常'
    if anomaly_count > 20:
        sec5_content += f'，以下展示前 20 例高优先级异常'
    sec5_content += '。异常筛选标准：回归残差 |z| > 1.65 或 CR < 80% 或 CR > 120%。'

    sec5 = {
        'heading': '高优先级异常清单',
        'content': sec5_content,
    }
    if anomaly_rows:
        sec5['table'] = {
            'title': '高优先级异常人员',
            'columns': [label_col_name, '部门', '职级', '当前薪酬', '异常类型', '详情'],
            'rows': anomaly_rows,
        }
    sections.append(sec5)

    # ── Section 6: 三档调薪方案 ──
    sc = scenarios.get('scenarios', {})
    sc_a = sc.get('A', {})
    sc_b = sc.get('B', {})
    sc_c = sc.get('C', {})

    scenario_rows = []
    for key, s in [('A', sc_a), ('B', sc_b), ('C', sc_c)]:
        scenario_rows.append([
            f'方案 {key}',
            s.get('description', ''),
            str(s.get('count', 0)),
            _fmt(s.get('annual_budget')),
            _pct(s.get('avg_increase_pct')),
            _pct(s.get('post_cr_compliance')),
        ])

    rec_budget = _fmt(sc_b.get('annual_budget'))
    rec_count = sc_b.get('count', 0)
    rec_compliance = _pct(sc_b.get('post_cr_compliance'))

    sections.append({
        'heading': '三档调薪方案',
        'table': {
            'title': '三方案对比',
            'columns': ['方案', '范围说明', '覆盖人数', '年度预算', '平均调幅', '调后CR合规率'],
            'rows': scenario_rows,
        },
        'highlight': f'推荐方案 B：覆盖 {rec_count} 人，年度预算 {rec_budget} 元，调后 CR 合规率提升至 {rec_compliance}。兼顾成本可控与公平性改善效果。',
        'content': f'**方案 A**（保守）：仅修复最严重异常（CR<80% 且回归异常），投入最少但改善有限。\n\n**方案 B**（推荐）：修复严重 + 中度偏离（CR<80%→P25，CR 80-90%→P40），性价比最优。\n\n**方案 C**（激进）：全员对齐至 CR≥90%，投入最大但公平性改善最彻底。',
    })

    # ── Section 7: ROI 测算 ──
    roi = scenarios.get('roi', {})
    roi_metrics = [
        {'label': '方案B年度投入', 'value': f'{_fmt(roi.get("investment_scenario_b"))} 元', 'state': 'neutral'},
        {'label': '潜在年度损失', 'value': f'{_fmt(roi.get("potential_annual_loss"))} 元', 'state': 'warn'},
        {'label': '1年期ROI', 'value': _pct(roi.get('roi_1yr_pct')), 'state': 'good' if (roi.get('roi_1yr_pct') or 0) > 0 else 'warn'},
    ]
    if roi.get('roi_2yr_pct') is not None:
        roi_metrics.append({
            'label': '2年期ROI',
            'value': _pct(roi.get('roi_2yr_pct')),
            'state': 'good' if roi.get('roi_2yr_pct', 0) > 0 else 'warn',
        })

    at_risk = roi.get('at_risk_count', 0)
    avg_sal = _fmt(roi.get('avg_salary'))
    roi_content = f'**测算假设：**\n'
    roi_content += f'- 高风险员工（CR<80%）：{at_risk} 人，平均薪酬 {avg_sal} 元/月\n'
    roi_content += f'- 核心人才替换成本：{roi.get("replacement_cost_assumption", "1.5倍年薪")}\n'
    roi_content += f'- 高风险员工年度流失概率：30%\n\n'
    roi_content += f'不采取行动的潜在年度损失为 {_fmt(roi.get("potential_annual_loss"))} 元，'
    roi_content += f'方案 B 投入 {_fmt(roi.get("investment_scenario_b"))} 元即可有效降低核心人才流失风险。'

    sections.append({
        'heading': 'ROI 测算',
        'metrics': roi_metrics,
        'content': roi_content,
    })

    # ── Section 8: 实施路线图 ──
    # Dynamic adjustments based on diagnosis findings
    phase1_actions = ['确认调薪名单和金额', '获取管理层审批', '准备薪酬沟通话术']
    phase2_actions = ['分批次执行调薪（优先高风险人员）', '一对一沟通薪酬调整', '同步更新薪酬系统']
    phase3_actions = ['调后 3 个月复盘 CR 分布变化', '跟踪调薪员工留存率', '评估公平性改善效果']
    phase4_actions = []

    if inv_rate is not None and inv_rate > 10:
        phase4_actions.append('建立市场薪酬对标机制，定期更新薪酬竞争力数据')
    if cr_rate is not None and cr_rate < 70:
        phase4_actions.append('完善薪酬带宽体系，明确各职级薪酬区间上下限')
    if gini_val is not None and gini_val > 0.3:
        phase4_actions.append('优化薪酬结构，缩小同岗位内部薪酬离散度')
    if r2_val is not None and r2_val < 0.3:
        phase4_actions.append('规范职级体系与薪酬挂钩机制')
    if not phase4_actions:
        phase4_actions.append('持续监控薪酬公平性指标')

    roadmap_content = f'**第一阶段（第 1-2 周）：审批与准备**\n'
    roadmap_content += '\n'.join(f'- {a}' for a in phase1_actions)
    roadmap_content += f'\n\n**第二阶段（第 3-4 周）：执行调薪**\n'
    roadmap_content += '\n'.join(f'- {a}' for a in phase2_actions)
    roadmap_content += f'\n\n**第三阶段（第 2-3 个月）：效果评估**\n'
    roadmap_content += '\n'.join(f'- {a}' for a in phase3_actions)
    roadmap_content += f'\n\n**第四阶段（持续）：制度优化**\n'
    roadmap_content += '\n'.join(f'- {a}' for a in phase4_actions)

    sections.append({
        'heading': '实施路线图',
        'content': roadmap_content,
        'items': phase1_actions + phase2_actions + phase3_actions + phase4_actions,
    })

    # ── Section 9: 制度建设建议 ──
    suggestions = [
        '建立年度薪酬公平性审计制度，定期输出诊断报告',
        '完善岗位价值评估体系，为薪酬定级提供客观依据',
        '建立薪酬调整审批流程，确保每次调薪有据可查',
    ]
    if inv_rate is not None and inv_rate > 10:
        suggestions.append('引入市场薪酬数据对标机制，每半年更新一次外部竞争力分析')
    if cr_rate is not None and cr_rate < 70:
        suggestions.append('优化薪酬带宽设计，为每个职级设定明确的薪酬区间（P25-P75）')
    if gini_val is not None and gini_val > 0.3:
        suggestions.append('强化同岗位内部薪酬一致性管理，定期识别并纠正异常偏离')
    if r2_val is not None and r2_val < 0.3:
        suggestions.append('完善职级晋升与薪酬联动机制，提高职级对薪酬的解释力')
    suggestions.append('建立员工薪酬沟通机制，提升薪酬透明度和员工满意度')

    sections.append({
        'heading': '制度建设建议',
        'items': suggestions,
    })

    # Write to file
    output_path = _os.path.join(_ANALYSIS_DIR, '_report_sections.json')
    try:
        _os.makedirs(_ANALYSIS_DIR, exist_ok=True)
        with open(output_path, 'w', encoding='utf-8') as f:
            _json.dump(sections, f, ensure_ascii=False, indent=2, default=str)
    except Exception as e:
        return {'error': f'Failed to write sections: {e}'}

    # Return relative path for generate_report source parameter
    rel_path = _os.path.relpath(output_path)

    print(f"[report_sections] Built {len(sections)} sections -> {rel_path}")
    return {
        'sections': sections,
        'file_path': rel_path,
    }

# ============================================================
# Data Flow Helpers
# ============================================================

def _preview(n=5):
    """Quick preview of current _df shape and first N rows."""
    df = globals().get('_df')
    if df is None:
        print("_df not loaded")
        return
    print(f"Current data: {df.shape[0]} rows x {df.shape[1]} cols")
    print(f"Columns: {list(df.columns)}")
    print(df.head(n).to_string())

def _data_status():
    """Show current data status: original vs current, row diff, available step snapshots."""
    df = globals().get('_df')
    df_raw = globals().get('_df_raw')
    step = globals().get('_CURRENT_STEP', '?')
    print(f"=== Data Status (Step {step}) ===")
    if df_raw is not None:
        print(f"Original: {df_raw.shape[0]} rows x {df_raw.shape[1]} cols")
    if df is not None:
        print(f"Current:  {df.shape[0]} rows x {df.shape[1]} cols")
        if df_raw is not None:
            diff = df_raw.shape[0] - df.shape[0]
            if diff > 0:
                print(f"Excluded: {diff} rows")
    snaps = []
    for i in range(6):
        p = _os.path.join(_ANALYSIS_DIR, f'_step{i}_df.pkl')
        if _os.path.exists(p):
            snaps.append(f"Step {i}")
    if snaps:
        print(f"Rollback available: {', '.join(snaps)}")

def _reset_to_original():
    """Reset _df to original data."""
    orig = _os.path.join(_ANALYSIS_DIR, '_original.pkl')
    if _os.path.exists(orig):
        globals()['_df'] = _pkl.load(open(orig, 'rb'))
        print(f"Reset to original: {globals()['_df'].shape[0]} rows")
    else:
        print("Original snapshot not found")

def _reset_to_step(step_num):
    """Reset _df to data from a specific step."""
    snap = _os.path.join(_ANALYSIS_DIR, f'_step{step_num}_df.pkl')
    if _os.path.exists(snap):
        globals()['_df'] = _pkl.load(open(snap, 'rb'))
        print(f"Reset to Step {step_num}: {globals()['_df'].shape[0]} rows")
    else:
        print(f"Step {step_num} snapshot not found")

def _export_current(filename='current_data', title='Current Data', format='excel'):
    """Export current _df to file (solves export_data tool not using _df)."""
    df = globals().get('_df')
    if df is None:
        print("_df not loaded, cannot export")
        return
    _export_detail(df, filename, title, format=format)
"###;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analysis_utils_contains_key_functions() {
        assert!(ANALYSIS_UTILS.contains("def _detect_columns(df)"));
        assert!(ANALYSIS_UTILS.contains("def _normalize_salary(series)"));
        assert!(ANALYSIS_UTILS.contains("def _step1_clean(df"));
        assert!(ANALYSIS_UTILS.contains("def _step4_diagnose(df"));
        assert!(ANALYSIS_UTILS.contains("def _step5_scenarios(df"));
        assert!(ANALYSIS_UTILS.contains("def _step5_build_report_sections(df"));
        assert!(ANALYSIS_UTILS.contains("def _step2_normalize(df"));
        assert!(ANALYSIS_UTILS.contains("def _step3_grading(df"));
        assert!(ANALYSIS_UTILS.contains("def _validate_step3(df"));
        assert!(ANALYSIS_UTILS.contains("def _validate_step4(df"));
    }

    #[test]
    fn test_analysis_utils_contains_helper_functions() {
        assert!(ANALYSIS_UTILS.contains("def _calc_cv(series)"));
        assert!(ANALYSIS_UTILS.contains("def _calc_gini(series)"));
        assert!(ANALYSIS_UTILS.contains("def _calc_compa_ratio("));
        assert!(ANALYSIS_UTILS.contains("def _detect_outliers_iqr("));
        assert!(ANALYSIS_UTILS.contains("def _salary_stats("));
        assert!(ANALYSIS_UTILS.contains("def _tenure_years("));
    }

    #[test]
    fn test_analysis_utils_contains_dimension_functions() {
        assert!(ANALYSIS_UTILS.contains("def _dim1_internal_equity("));
        assert!(ANALYSIS_UTILS.contains("def _dim2_cross_position("));
        assert!(ANALYSIS_UTILS.contains("def _dim3_regression("));
        assert!(ANALYSIS_UTILS.contains("def _dim4_inversion("));
        assert!(ANALYSIS_UTILS.contains("def _dim5_structure_fit("));
        assert!(ANALYSIS_UTILS.contains("def _dim6_compa_ratio("));
    }

    #[test]
    fn test_analysis_utils_contains_data_flow_helpers() {
        assert!(ANALYSIS_UTILS.contains("def _preview("));
        assert!(ANALYSIS_UTILS.contains("def _data_status("));
        assert!(ANALYSIS_UTILS.contains("def _reset_to_original("));
        assert!(ANALYSIS_UTILS.contains("def _reset_to_step("));
        assert!(ANALYSIS_UTILS.contains("def _export_current("));
    }

    #[test]
    fn test_step1_clean_auto_updates_df() {
        assert!(ANALYSIS_UTILS.contains("globals()['_df'] = retained_df"));
    }

    #[test]
    fn test_analysis_utils_contains_cache_functions() {
        assert!(ANALYSIS_UTILS.contains("def _cache_result("));
        assert!(ANALYSIS_UTILS.contains("def _load_cached("));
        assert!(ANALYSIS_UTILS.contains("_ANALYSIS_DIR"));
    }

    #[test]
    fn test_analysis_utils_no_forbidden_imports() {
        // Verify no dangerous imports are included
        assert!(!ANALYSIS_UTILS.contains("import subprocess"));
        assert!(!ANALYSIS_UTILS.contains("import multiprocessing"));
        assert!(!ANALYSIS_UTILS.contains("import ctypes"));
    }
}
