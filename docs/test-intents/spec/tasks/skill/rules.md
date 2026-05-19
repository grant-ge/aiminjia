# rules.md — 技能（Skill） 意图测试规格

## 测试范围

覆盖无状态 Skill 系统的端到端行为：从本地 `~/.renlijia/skills/` 与 `~/.renlijia/users/{scope}/skills/` 目录下 SKILL.md 的加载与 frontmatter 解析、技能目录在对话 turn 中的 prompt 注入（catalog_prompt）、对话中 LLM 通过 `load_skill` 工具按需加载 SKILL.md body，到技能草稿（skill draft）的编辑、校验、发布上线（覆盖原 skill 目录）流程。关注 `plugin/skill/`、`commands/skill_management.rs`、前端 `src/features/skill-center/` 的一致性。

## 待覆盖的主要场景

- 场景 1：`~/.renlijia/skills/foo/SKILL.md` 存在时，新对话 turn 的 system prompt 中包含该技能的 catalog 条目（名称 + description）
- 场景 2：LLM 在对话中调用 `load_skill` 工具，工具返回该 SKILL.md 的 body 文本（无状态加载，不污染后续 turn）
- 场景 3：SKILL.md frontmatter 字段缺失或非法（缺 name / description 等）时 loader 跳过该 skill 并 warn，不影响其他 skill 加载
- 场景 4：用户在 Skill Center 创建草稿（skill draft），保存后只写到草稿目录，原 skill 不受影响
- 场景 5：草稿发布（publish）后 `~/.renlijia/skills/{id}/SKILL.md` 被原子替换，catalog 在下一次 turn 立即反映新版本
- 场景 6：发布前校验失败（如 SKILL.md body 缺正文、frontmatter 解析失败）时阻断发布，给出错误信息
- 场景 7：技能 `updated_at` 在发布后正确更新，前端列表按更新时间排序，新发布的排在前面
- 场景 8：全局 skills 与 user-scoped skills 同名时按既定优先级合并/覆盖，loader 不重复注入两个同名条目

## 待补充

> 具体意图（场景/前提/操作/验收标准）待补全。
