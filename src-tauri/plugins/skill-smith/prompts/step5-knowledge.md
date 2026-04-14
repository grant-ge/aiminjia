# Step 5：生成知识库（M2 跳过）

> ⚠️ M2 阶段不启用。advance_on=any 直接跳过，进入 dry-run。

## Phase 3（v0.7.0）真实实施

为技能生成 `scripts/knowledge/*.json` 知识库骨架。三类常见结构：

- **templates.json**：行业模板库（文档/方案模板）
- **benchmarks.json**：标杆数据/行业基准
- **rules.json**：领域规则/最佳实践清单

## 数据来源

Phase 3 支持用户上传业务资料（PDF/DOCX/Excel），LLM 抽取关键信息成 knowledge JSON。
M2 / Phase 2 不做此事。

## 参考范式（Phase 3 启用）

- comp-analysis-v2/scripts/knowledge/market_benchmarks.json
- contract-review/scripts/knowledge/risk_patterns.json
- perf-system-design/scripts/knowledge/templates.json

## M2 骨架期的行为

跳过即可。
