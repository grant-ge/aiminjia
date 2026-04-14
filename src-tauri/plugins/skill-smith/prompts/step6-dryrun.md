# Step 6：Dry-run 校验

> ⚠️ M2 骨架阶段：此 prompt 为占位。本步会真正用到 T7 的
> `dry_run_skill_draft` 命令，M3 阶段接入 LLM 后就能一键触发。

## 本步目标（M3 真实实施）

调用 `dry_run_skill_draft(draftId)` 跑一遍 6 项静态检查：

1. **schema** — 所有字段合法
2. **prompts-reference** — workflow 引用的 .md 文件都在
3. **prompts-content** — 每个 prompt 非空且 ≥50 字节
4. **python-scripts** — M2/Phase 2 前跳过
5. **knowledge** — JSON 语法检查
6. **loadable** — 真实 parse_plugin_manifest + DeclarativeSkill::load

## 报告处理

- 若 `pass=true`（可能有 warn / skip）→ 向用户展示"技能已就绪"卡片 + 本步通过的 check 清单 + 自动进入 step7
- 若 `pass=false` → 展示 fail 清单 + `fix_hint`，让 LLM 自动回到对应 step 修复

## 进阶

Phase 2 后，dry-run 会包含真实 Python 沙箱执行（用内置 sample 数据），
给出 precompute 输出预览。Phase 3 后，可用用户自己的业务数据跑一次。

## M2 骨架期的行为

M2 没接 LLM，直接给用户一句话："模拟 dry-run 通过，点击下一步查看交付选项。"
