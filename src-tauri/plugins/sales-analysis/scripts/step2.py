import json as _json_mod
import os as _os_mod

result = {}
try:
    _playbooks = _KNOWLEDGE.get('playbooks', {}) if '_KNOWLEDGE' in dir() else {}

    if '_df' in dir() and _df is not None:
        df = _df.copy()
        cols = list(df.columns)
        amount_cols = [c for c in cols if any(k in c.lower() for k in ['金额', '收入', '销售额', 'amount', 'revenue', 'gmv', 'sales'])]
        customer_cols = [c for c in cols if any(k in c.lower() for k in ['客户', '买家', 'customer', 'buyer', 'client'])]
        product_cols = [c for c in cols if any(k in c.lower() for k in ['产品', '品类', 'product', 'category', 'sku', 'item'])]

        summary = {}

        # Pareto analysis (80/20)
        if customer_cols and amount_cols:
            cc, ac = customer_cols[0], amount_cols[0]
            df[ac] = pd.to_numeric(df[ac], errors='coerce')
            cust_rev = df.groupby(cc)[ac].sum().sort_values(ascending=False)
            total = cust_rev.sum()
            cumsum = cust_rev.cumsum()
            pct_80 = (cumsum <= total * 0.8).sum()
            summary['pareto'] = {
                'total_customers': len(cust_rev),
                'top_20pct_count': max(1, int(len(cust_rev) * 0.2)),
                'top_20pct_revenue_share': float(cust_rev.head(max(1, int(len(cust_rev) * 0.2))).sum() / total) if total > 0 else 0,
                'customers_for_80pct_revenue': int(pct_80),
            }
            # Customer concentration
            top5_share = float(cust_rev.head(5).sum() / total) if total > 0 else 0
            summary['customer_concentration'] = {
                'top5_revenue_share': top5_share,
                'risk': '高' if top5_share > 0.5 else '中' if top5_share > 0.3 else '低'
            }

        # Product mix
        if product_cols and amount_cols:
            pc, ac = product_cols[0], amount_cols[0]
            df[ac] = pd.to_numeric(df[ac], errors='coerce')
            prod_rev = df.groupby(pc)[ac].sum().sort_values(ascending=False)
            summary['product_mix'] = {str(k): float(v) for k, v in prod_rev.head(10).items()}

        result = summary
        if _playbooks:
            result['growth_playbooks'] = _playbooks
    else:
        result = {'error': '未找到数据'}
except Exception as e:
    result = {'error': str(e)}

_cache_result('sales_step2', result)
with open(_os_mod.path.join(_ANALYSIS_DIR, 'step2_precompute.json'), 'w', encoding='utf-8') as f:
    _json_mod.dump(result, f, ensure_ascii=False, default=str, indent=2)
print(_json_mod.dumps(result, ensure_ascii=False, default=str, indent=2))
