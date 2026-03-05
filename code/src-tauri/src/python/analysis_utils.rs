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

    return results

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
    return results

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
