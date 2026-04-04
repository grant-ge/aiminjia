import json as _json_mod
import os as _os_mod

result = {}
try:
    _benchmarks = _KNOWLEDGE.get('benchmarks', {}) if '_KNOWLEDGE' in dir() else {}

    if '_df' in dir() and _df is not None:
        df = _df.copy()
        cols = list(df.columns)

        question_stats = {}
        nps_result = None
        overall_scores = {}

        for c in cols:
            series = df[c].dropna()
            if len(series) == 0:
                continue

            nunique = series.nunique()
            numeric = pd.to_numeric(series, errors='coerce')
            numeric_valid = numeric.dropna()
            is_numeric = len(numeric_valid) > len(series) * 0.7

            if is_numeric and len(numeric_valid) > 0:
                vmin, vmax = float(numeric_valid.min()), float(numeric_valid.max())

                # NPS calculation (0-10)
                if vmin >= 0 and vmax <= 10 and nunique <= 11:
                    promoters = int((numeric_valid >= 9).sum())
                    passives = int(((numeric_valid >= 7) & (numeric_valid < 9)).sum())
                    detractors = int((numeric_valid < 7).sum())
                    total = len(numeric_valid)
                    nps_score = round((promoters - detractors) / total * 100, 1) if total > 0 else 0
                    nps_result = {
                        'promoters': promoters,
                        'passives': passives,
                        'detractors': detractors,
                        'total': total,
                        'nps_score': nps_score,
                        'column': c,
                    }
                    question_stats[c] = {'type': 'nps', 'nps': nps_result}

                # Likert (1-5 or 1-7)
                elif vmin >= 1 and vmax <= 7 and nunique <= 7:
                    mean_val = float(numeric_valid.mean())
                    median_val = float(numeric_valid.median())
                    std_val = float(numeric_valid.std())
                    top2 = float((numeric_valid >= vmax - 1).sum() / len(numeric_valid))
                    bottom2 = float((numeric_valid <= vmin + 1).sum() / len(numeric_valid))
                    question_stats[c] = {
                        'type': 'likert',
                        'mean': round(mean_val, 2),
                        'median': median_val,
                        'std': round(std_val, 2),
                        'top2box': round(top2, 4),
                        'bottom2box': round(bottom2, 4),
                    }
                    overall_scores[c] = round(mean_val, 2)
                else:
                    question_stats[c] = {
                        'type': 'numeric',
                        'mean': round(float(numeric_valid.mean()), 2),
                        'median': float(numeric_valid.median()),
                        'std': round(float(numeric_valid.std()), 2),
                    }
            elif nunique <= 10:
                # Single choice - frequency distribution
                freq = series.value_counts()
                question_stats[c] = {
                    'type': 'single_choice',
                    'distribution': {str(k): int(v) for k, v in freq.items()},
                    'distribution_pct': {str(k): round(float(v / len(series)), 4) for k, v in freq.items()},
                }

        result = {
            'question_stats': question_stats,
            'overall_scores': overall_scores,
        }
        if nps_result:
            result['nps_result'] = nps_result
        if _benchmarks:
            result['industry_benchmarks'] = _benchmarks
    else:
        result = {'error': '未找到数据'}
except Exception as e:
    result = {'error': str(e)}

_cache_result('survey_step1', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step1_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, default=str, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, default=str, indent=2))
