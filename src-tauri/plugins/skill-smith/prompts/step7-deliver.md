# Step 7：交付（安装 / 导出）

> ⚠️ M2 骨架阶段：此 prompt 为占位。T4 已实现真实 commit / export 命令。

## 本步目标（M3 真实实施）

向用户展示两个交付选项：

🔵 **立即安装** — 调用 `commit_skill_draft(draftId)`
- 冲突时（`conflict=true`）弹 modal："已存在同名技能，要覆盖 / 改名 / 取消？"
- 覆盖 → `commit_skill_draft_force`
- 改名 → 回到 step1 改 plugin.id → 重新 dry-run → 回到本步
- 成功后 → Toast："已安装到 custom_plugins/{id}，立即可用"

📦 **导出 .aijia-skill 包** — 调用 `export_skill_draft(draftId, outputDir)`
- 先弹 Tauri 文件对话框让用户选目录
- 成功后 → Toast："已导出到 {path}，可以发给同事或自己留存"
- 导出不清理 draft，用户还能再安装/再导出

## 收尾话术（两种都支持）

> "🎉 你的技能已经完成了。以后想分享给同事，随时来技能管理页面导出 .aijia-skill 包。
> 试试说 `{trigger_text}` 就能触发这个技能。"

## M2 骨架期的行为

直接给用户一句话："M2 骨架测试完成。真实流程在 M3 完成后可用。"
