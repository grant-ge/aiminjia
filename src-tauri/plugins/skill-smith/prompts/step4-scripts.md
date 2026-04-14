# Step 4：生成 Python 脚本（M2 跳过）

> ⚠️ M2 阶段不启用。直接 advance_on=any 跳过本步，提示用户"纯对话型技能已完成生成，下一步进入 dry-run"。

## Phase 2（v0.6.0）真实实施

如果 workflow 里有 step 定义了 `precompute = "scripts/stepN.py"`，本步负责
生成对应的 Python 脚本：

- 白名单库：pandas / openpyxl / python-docx / PyPDF2 / matplotlib
- 必须通过 AST 审查（禁止 os.system / subprocess / socket / eval / exec）
- 沙箱 dry-run（10s CPU / 512MB RAM）
- precompute 输出是给 LLM 看的 `_precompute_result` 字典

## 参考范式（Phase 2 启用）

- 数据读取 → comp-analysis-v2/scripts/step0.py
- 分组聚合 → sales-analysis/scripts/step0.py
- 问卷处理 → survey-analysis/scripts/step0.py

## M2 骨架期的行为

跳过即可。本步 advance_on 是 "any"，用户发任何消息都自动进入 step5。
