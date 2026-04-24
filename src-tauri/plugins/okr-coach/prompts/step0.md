# Step 0: 背景与目标确认

## 目标

了解 OKR 制定的层级、业务背景、战略重点和周期，为 OKR 初稿制定做好准备。

## 成功标准

1. OKR 层级已确认（公司/部门/团队/个人）
2. 业务背景和战略重点已明确
3. OKR 周期已确认（季度/半年/年度）
4. 如有上级 OKR，已加载并理解对齐要求
5. 背景信息已通过 `save_analysis_note` 记录

## 执行步骤

[precompute_result] 已加载 OKR 知识库，包含：OKR 原则（objective_rules / key_result_rules / common_mistakes / scoring）、各职能 OKR 案例库（okr_examples）、指标库（metrics_library）。在后续推荐 O/KR 时，优先参考知识库中的原则和案例，结合用户实际情况个性化调整；推荐 KR 指标时从 metrics_library 中匹配对应职能的常用指标。

1. 了解 OKR 制定层级：公司/部门/团队/个人
2. 了解业务背景和战略重点
3. 如有上级 OKR，使用 `load_file` 加载
4. 确认周期：季度/半年/年度
5. 使用 `save_analysis_note` 记录背景

确认后进入下一步。

## 约束

- 不主动假设层级和周期，必须由用户确认
- 上级 OKR 缺失时不强制要求，但提醒对齐的重要性

## 异常处理

- **precompute 出错**：跳过知识库匹配，直接基于 OKR 核心原则开展背景确认
- **信息不足**：列出已收集和缺失的背景信息，引导用户逐项补充
- **用户修改需求**：更新 `save_analysis_note` 记录，以最新确认为准
