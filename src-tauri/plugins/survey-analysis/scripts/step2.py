import json as _json_mod
import os as _os_mod

result = {}
try:
    _playbooks = _KNOWLEDGE.get('action_playbooks', {}) if '_KNOWLEDGE' in dir() else {}

    if '_df' in dir() and _df is not None:
        df = _df.copy()
        cols = list(df.columns)

        # Detect demographic columns
        demo_cols = [c for c in cols if any(k in c.lower() for k in [
            '性别', '年龄', '部门', '区域', '地区', '学历', '职级', '岗位',
            'gender', 'age', 'department', 'region', 'education', 'level', 'position'
        ])]

        # Detect satisfaction/score columns (likert or NPS)
        score_cols = []
        for c in cols:
            series = df[c].dropna()
            numeric = pd.to_numeric(series, errors='coerce')
            numeric_valid = numeric.dropna()
            if len(numeric_valid) > len(series) * 0.7 and len(numeric_valid) > 0:
                vmin, vmax = float(numeric_valid.min()), float(numeric_valid.max())
                if (vmin >= 1 and vmax <= 7 and numeric_valid.nunique() <= 7) or (vmin >= 0 and vmax <= 10 and numeric_valid.nunique() <= 11):
                    score_cols.append(c)

        cross_analysis = {}
        top_groups = {}
        bottom_groups = {}

        if demo_cols and score_cols:
            for dc in demo_cols[:3]:
                for sc in score_cols[:5]:
                    df[sc] = pd.to_numeric(df[sc], errors='coerce')
                    group_means = df.groupby(dc)[sc].mean().sort_values(ascending=False)
                    group_means_dict = {str(k): round(float(v), 2) for k, v in group_means.items()}

                    key = f'{dc}_x_{sc}'
                    cross_analysis[key] = group_means_dict

                    # Top and bottom groups
                    if len(group_means) >= 2:
                        top_groups[key] = {str(group_means.index[0]): round(float(group_means.iloc[0]), 2)}
                        bottom_groups[key] = {str(group_means.index[-1]): round(float(group_means.iloc[-1]), 2)}

        summary = {
            'cross_analysis': cross_analysis,
            'top_groups': top_groups,
            'bottom_groups': bottom_groups,
            'demographic_columns': demo_cols,
            'score_columns': score_cols,
        }

        if _playbooks:
            summary['action_playbooks'] = _playbooks

        result = summary
    else:
        result = {'error': '未找到数据'}
except Exception as e:
    result = {'error': str(e)}

_cache_result('survey_step2', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step2_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, default=str, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, default=str, indent=2))
