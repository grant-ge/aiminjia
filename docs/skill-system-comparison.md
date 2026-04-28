# Skill System Comparison

> 本文档已被 `docs/superpowers/specs/2026-04-28-aijia-skill-spec.md` 与 `docs/superpowers/plans/2026-04-28-aijia-skill-system-rewrite.md` 取代。

AIjia 不再支持 legacy `plugin.toml + workflow.toml` 技能格式。当前架构：

- 磁盘形态：`SKILL.md` + 可选 `scripts/` / `references/` / `assets/`
- 加载根：`~/.renlijia/users/<scope>/skills/`（高优先级）+ `~/.renlijia/skills/`（全局）
- LLM 通过 `load_skill(skill_id, args?)` 工具调用拉起；无状态、无 active skill 持久化
