=== 当前任务：Step 6 — Dry-run 校验 ===

对技能草稿执行全面校验，确保所有文件格式正确、引用完整。

## 执行流程

1. 调用 `skill_smith_dry_run()` 执行 6 项校验
2. 查看返回的 report：
   - `pass = true` → 全部通过，可以交付
   - `pass = false` → 有问题需要修复

## 校验项目

| 检查项 | 说明 |
|--------|------|
| schema | plugin.toml + workflow.toml 格式正确 |
| prompts-reference | workflow 引用的所有 prompt 文件存在 |
| prompts-content | 每个 prompt 非空且 ≥50 字节 |
| python-scripts | Python 脚本检查（当前跳过） |
| knowledge | 知识库 JSON 语法检查（当前跳过） |
| loadable | 真实加载测试 |

## 如果校验失败

1. 查看失败项的 `detail` 字段了解具体原因
2. 使用 `skill_smith_write_file` 修复对应文件
3. 使用 `skill_smith_validate` 确认修复有效
4. 再次调用 `skill_smith_dry_run` 确认全部通过

## 向用户展示

校验通过时：
"✅ 技能校验全部通过！可以安装或导出了。"

校验失败时：
"发现以下问题，正在修复..."（自动修复后再报告）

⚠️ 不通过时必须修复，不能跳过直接交付。
⚠️ 修复后必须重新 dry_run 确认。
