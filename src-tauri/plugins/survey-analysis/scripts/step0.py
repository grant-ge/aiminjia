import json as _json_mod
import os as _os_mod

result = {}
try:
    if '_df' in dir() and _df is not None:
        cols = list(_df.columns)
        dtypes = {c: str(_df[c].dtype) for c in cols}
        row_count = len(_df)
        sample_size = row_count

        # Detect question types
        question_types = {}
        for c in cols:
            series = _df[c].dropna()
            if len(series) == 0:
                question_types[c] = 'empty'
                continue

            nunique = series.nunique()
            # Check if numeric
            numeric = pd.to_numeric(series, errors='coerce')
            numeric_valid = numeric.dropna()
            is_numeric = len(numeric_valid) > len(series) * 0.7

            if is_numeric and len(numeric_valid) > 0:
                vmin, vmax = float(numeric_valid.min()), float(numeric_valid.max())
                # NPS: 0-10 range
                if vmin >= 0 and vmax <= 10 and nunique <= 11:
                    question_types[c] = 'nps'
                # Likert: 1-5 or 1-7 range
                elif vmin >= 1 and vmax <= 7 and nunique <= 7:
                    question_types[c] = 'likert'
                else:
                    question_types[c] = 'numeric'
            elif nunique <= 10:
                question_types[c] = 'single_choice'
            elif any(d in str(series.iloc[0]) for d in [',', '、', ';', '|']):
                question_types[c] = 'multi_choice'
            elif series.astype(str).str.len().mean() > 20:
                question_types[c] = 'open_text'
            else:
                question_types[c] = 'single_choice'

        # Detect survey type
        col_text = ' '.join([c.lower() for c in cols])
        survey_type = 'general'
        if any(k in col_text for k in ['nps', '推荐', 'recommend']):
            survey_type = 'NPS'
        elif any(k in col_text for k in ['满意', 'satisfaction', '评分']):
            survey_type = 'satisfaction'
        elif any(k in col_text for k in ['员工', 'employee', '敬业', '组织']):
            survey_type = 'employee'
        elif any(k in col_text for k in ['市场', 'market', '品牌', 'brand', '偏好']):
            survey_type = 'market_research'

        # Completion rate
        completion_rate = float((_df.notna().sum(axis=1) / len(cols)).mean())

        result = {
            'question_types': question_types,
            'sample_size': sample_size,
            'survey_type': survey_type,
            'completion_rate': round(completion_rate, 4),
            'columns': cols,
            'dtypes': dtypes,
            'sample_values': {c: str(_df[c].iloc[0]) if len(_df) > 0 else '' for c in cols[:10]}
        }
    else:
        result = {'error': '未找到数据文件'}
except Exception as e:
    result = {'error': str(e)}

_cache_result('survey_step0', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step0_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, default=str, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, default=str, indent=2))
