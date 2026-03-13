# Step 2: 9-box grid placement and distribution
# Executed automatically by Rust before LLM starts.
# Depends on: _df, _ANALYSIS_DIR, _export_detail

import json as _json_mod
import pandas as _pd_mod

# 9-box labels: performance(row) x potential(col)
_9box_labels = {
    (3, 3): '明星人才', (3, 2): '核心骨干', (3, 1): '专业专家',
    (2, 3): '高潜新星', (2, 2): '稳定贡献者', (2, 1): '待发展者',
    (1, 3): '待激活者', (1, 2): '观察对象', (1, 1): '需改进者',
}

_9box_labels_en = {
    (3, 3): 'Star', (3, 2): 'Core Player', (3, 1): 'Expert',
    (2, 3): 'Rising Star', (2, 2): 'Steady Contributor', (2, 1): 'Needs Development',
    (1, 3): 'Underperformer w/ Potential', (1, 2): 'Watch List', (1, 1): 'Underperformer',
}

# _df columns don't persist across steps — must recompute from step1 cache
try:
    step1_path = os.path.join(_ANALYSIS_DIR, 'step1_precompute.json')
    with open(step1_path, 'r') as f:
        step1 = _json_mod.load(f)
except (FileNotFoundError, _json_mod.JSONDecodeError):
    step1 = {}

perf_col = step1.get('perf_col')
pot_col = step1.get('pot_col')
perf_thresholds = step1.get('performance', {}).get('thresholds', {})
pot_thresholds = step1.get('potential', {}).get('thresholds', {})

# Recompute normalized levels from raw scores
can_proceed = False
if perf_col and pot_col and perf_col in _df.columns and pot_col in _df.columns:
    perf_numeric = _pd_mod.to_numeric(_df[perf_col], errors='coerce')
    pot_numeric = _pd_mod.to_numeric(_df[pot_col], errors='coerce')

    # If not numeric, try text level mapping
    def _to_numeric_fallback(series):
        numeric = _pd_mod.to_numeric(series, errors='coerce')
        if numeric.notna().sum() > 0:
            return numeric
        level_map = {
            'A': 3, 'B': 2, 'C': 1, 'D': 0,
            '优': 3, '良': 2, '中': 1.5, '差': 1, '不合格': 0,
            '优秀': 3, '良好': 2, '合格': 1, '待改进': 0,
            '高': 3, '中': 2, '低': 1,
            'high': 3, 'medium': 2, 'low': 1,
        }
        return series.astype(str).str.strip().map(level_map)

    perf_numeric = _to_numeric_fallback(_df[perf_col])
    pot_numeric = _to_numeric_fallback(_df[pot_col])

    # Normalize using cached thresholds or recalculate
    def _apply_3level(series, thresholds):
        if not thresholds:
            valid = series.dropna()
            if len(valid) == 0:
                return series
            q33 = float(valid.quantile(0.33))
            q66 = float(valid.quantile(0.66))
        else:
            valid = series.dropna()
            if len(valid) == 0:
                return series
            q33 = thresholds.get('low_max', float(valid.quantile(0.33)))
            q66 = thresholds.get('mid_max', float(valid.quantile(0.66)))
        return series.apply(lambda x: 1 if x <= q33 else (2 if x <= q66 else 3) if _pd_mod.notna(x) else x)

    _df['_perf_level'] = _apply_3level(perf_numeric, perf_thresholds)
    _df['_pot_level'] = _apply_3level(pot_numeric, pot_thresholds)
    can_proceed = _df['_perf_level'].notna().sum() > 0 and _df['_pot_level'].notna().sum() > 0

if not can_proceed:
    _precompute = {'error': '绩效/潜力归一化数据缺失，请返回Step 1重新处理'}
    with open(os.path.join(_ANALYSIS_DIR, 'step2_precompute.json'), 'w') as f:
        _json_mod.dump(_precompute, f, ensure_ascii=False, default=str)
    print(_json_mod.dumps(_precompute, ensure_ascii=False))
else:
    # Assign 9-box position
    def _assign_label(row, label_dict):
        if _pd_mod.notna(row['_perf_level']) and _pd_mod.notna(row['_pot_level']):
            try:
                return label_dict.get((int(row['_perf_level']), int(row['_pot_level'])), '未分类')
            except (ValueError, TypeError):
                return '数据异常'
        return '数据缺失'

    _df['_9box_label'] = _df.apply(lambda row: _assign_label(row, _9box_labels), axis=1)
    _df['_9box_label_en'] = _df.apply(lambda row: _assign_label(row, _9box_labels_en), axis=1)

    # Build 9-box distribution matrix
    grid = {}
    total = len(_df)
    for (perf, pot), label in _9box_labels.items():
        count = int(((_df['_perf_level'] == perf) & (_df['_pot_level'] == pot)).sum())
        grid[label] = {
            'count': count,
            'percentage': round(count / total * 100, 1) if total > 0 else 0,
            'perf_level': perf,
            'pot_level': pot,
            'label_en': _9box_labels_en[(perf, pot)],
        }

    # Health metrics
    star_pct = grid.get('明星人才', {}).get('percentage', 0)
    risk_pct = sum(grid.get(l, {}).get('percentage', 0) for l in ['需改进者', '观察对象', '待激活者'])
    unclassified = int((_df['_9box_label'] == '数据缺失').sum()) + int((_df['_9box_label'] == '数据异常').sum())

    _precompute = {
        'grid': grid,
        'health': {
            'star_percentage': star_pct,
            'risk_percentage': risk_pct,
            'unclassified': unclassified,
        },
        'total_employees': total,
    }

    with open(os.path.join(_ANALYSIS_DIR, 'step2_precompute.json'), 'w') as f:
        _json_mod.dump(_precompute, f, ensure_ascii=False, default=str)
    print(_json_mod.dumps(_precompute, ensure_ascii=False, default=str, indent=2))

    _export_detail(_df, 'step2_9box_mapping', '九宫格定位明细')
