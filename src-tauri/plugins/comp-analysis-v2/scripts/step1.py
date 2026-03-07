# Step 1: Field detection + data cleaning
# Executed automatically by Rust before LLM starts.
# Depends on: _df, _ANALYSIS_DIR, _detect_columns, _step1_clean, _export_detail
# (all injected by file preamble + analysis preamble)

import json as _json_mod

col_map_result = _detect_columns(_df)
clean_result = _step1_clean(_df, col_map_result)

# Build detailed field mapping table for LLM display
detected = col_map_result.get('detected', {})
field_mapping = []
for semantic, column in detected.items():
    field_mapping.append({
        'semantic': semantic,
        'semantic_zh': {
            'id': '员工ID', 'department': '部门', 'position': '职位',
            'level': '职级', 'status': '状态', 'hire_date': '入职日期',
            'location': '工作地点', 'emp_type': '用工类型',
            'base_salary': '基本工资', 'gross': '应发工资', 'net': '实发工资'
        }.get(semantic, semantic),
        'column': column
    })

# Build detailed exclusion breakdown
exclusion_details = []
exclusion_summary = clean_result.get('exclusion_summary', {})
for reason, count in exclusion_summary.items():
    reason_zh = {
        'departed': '已离职', 'non_fulltime': '非全职',
        'probation': '试用期', 'current_month_hire': '当月入职',
        'zero_base_salary': '基本工资为0'
    }.get(reason, reason)
    exclusion_details.append({'reason': reason_zh, 'count': count})

# Cache precompute result for LLM display
_precompute = {
    'overview': {
        'total_rows': clean_result['overview']['rows'],
        'total_cols': clean_result['overview']['cols'],
        'retained': clean_result['total_retained'],
        'excluded': clean_result['total_excluded'],
    },
    'field_mapping': field_mapping,
    'exclusion_details': exclusion_details,
    'quality': clean_result.get('quality', {}),
}

with open(os.path.join(_ANALYSIS_DIR, 'step1_precompute.json'), 'w') as f:
    _json_mod.dump(_precompute, f, ensure_ascii=False, default=str)
print(_json_mod.dumps(_precompute, ensure_ascii=False, default=str, indent=2))

# Auto-export intermediate data
if clean_result.get('excluded_df') is not None and len(clean_result['excluded_df']) > 0:
    _export_detail(clean_result['excluded_df'], 'step1_exclusion_detail', '排除人员明细')
_export_detail(_df, 'step1_cleaned_data', '清洗后数据')
