# Step 1: Field detection + data cleaning for recruitment data
# Executed automatically by Rust before LLM starts.
# Depends on: _df, _ANALYSIS_DIR, _export_detail

import json as _json_mod
import pandas as _pd_mod

# --- Field detection ---
# Common recruitment field patterns
_recruitment_field_patterns = {
    'candidate_name': ['姓名', '候选人', 'name', 'candidate'],
    'position': ['岗位', '职位', '应聘职位', 'position', 'job_title', 'role'],
    'department': ['部门', '用人部门', 'department', 'dept'],
    'channel': ['渠道', '来源', '招聘渠道', 'source', 'channel'],
    'apply_date': ['投递日期', '申请日期', '投递时间', 'apply_date', 'application_date'],
    'resume_screen_date': ['筛选日期', '简历筛选', 'screen_date', 'resume_review'],
    'interview_date': ['面试日期', '初面日期', 'interview_date', 'first_interview'],
    'interview2_date': ['复面日期', '二面日期', 'second_interview', 'final_interview'],
    'offer_date': ['offer日期', '录用日期', 'offer_date'],
    'onboard_date': ['入职日期', '报到日期', 'onboard_date', 'start_date', 'hire_date'],
    'status': ['状态', '当前状态', '招聘状态', 'status', 'stage', '阶段'],
    'cost': ['费用', '成本', '渠道费用', 'cost', 'expense'],
    'probation_result': ['试用期结果', '转正', 'probation', 'probation_result'],
    'resign_date': ['离职日期', 'resign_date', 'termination_date'],
}

detected = {}
col_lower = {c: c.lower().strip() for c in _df.columns}

for semantic, patterns in _recruitment_field_patterns.items():
    for col, col_l in col_lower.items():
        for p in patterns:
            if p.lower() in col_l or col_l in p.lower():
                detected[semantic] = col
                break
        if semantic in detected:
            break

# --- Detect recruitment stages from status column ---
stages_detected = []
if 'status' in detected:
    status_col = detected['status']
    unique_statuses = _df[status_col].dropna().unique().tolist()
    stages_detected = unique_statuses

# --- Data cleaning ---
total_rows = len(_df)
excluded_rows = []
clean_df = _df.copy()

# Remove rows with no candidate identifier
if 'candidate_name' in detected:
    mask = clean_df[detected['candidate_name']].isna() | (clean_df[detected['candidate_name']].astype(str).str.strip() == '')
    excluded_rows.extend([{'reason': '候选人姓名为空', 'count': int(mask.sum())}])
    clean_df = clean_df[~mask]

# Remove duplicates
dup_count = clean_df.duplicated().sum()
if dup_count > 0:
    excluded_rows.append({'reason': '重复记录', 'count': int(dup_count)})
    clean_df = clean_df.drop_duplicates()

total_excluded = sum(e['count'] for e in excluded_rows)
total_retained = len(clean_df)

# --- Build field mapping table ---
field_mapping = []
semantic_zh = {
    'candidate_name': '候选人姓名', 'position': '应聘岗位', 'department': '用人部门',
    'channel': '招聘渠道', 'apply_date': '投递日期', 'resume_screen_date': '简历筛选日期',
    'interview_date': '面试日期', 'interview2_date': '复面日期', 'offer_date': 'Offer日期',
    'onboard_date': '入职日期', 'status': '招聘状态', 'cost': '渠道费用',
    'probation_result': '试用期结果', 'resign_date': '离职日期',
}
for sem, col in detected.items():
    field_mapping.append({
        'semantic': sem,
        'semantic_zh': semantic_zh.get(sem, sem),
        'column': col,
    })

# --- Quality check ---
quality = {}
for sem, col in detected.items():
    missing = int(clean_df[col].isna().sum())
    if missing > 0:
        pct = round(missing / len(clean_df) * 100, 1) if len(clean_df) > 0 else 0
        quality[semantic_zh.get(sem, sem)] = f'{missing}条缺失 ({pct}%)'

# --- Cache precompute result ---
_precompute = {
    'overview': {
        'total_rows': total_rows,
        'total_cols': len(_df.columns),
        'retained': total_retained,
        'excluded': total_excluded,
    },
    'field_mapping': field_mapping,
    'exclusion_details': excluded_rows,
    'stages_detected': stages_detected,
    'quality': quality,
}

with open(os.path.join(_ANALYSIS_DIR, 'step1_precompute.json'), 'w') as f:
    _json_mod.dump(_precompute, f, ensure_ascii=False, default=str)
print(_json_mod.dumps(_precompute, ensure_ascii=False, default=str, indent=2))

# Update _df to cleaned version
_df = clean_df

# Auto-export
_export_detail(_df, 'step1_cleaned_data', '清洗后招聘数据')
