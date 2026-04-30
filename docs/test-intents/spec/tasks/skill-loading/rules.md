# Skill loading rules

- Rule 1: Runtime 只加载用户级 (`~/.renlijia/users/<scope>/skills/`) 和全局 (`~/.renlijia/skills/`) 下的 SKILL.md skill 目录。
- Rule 2: 同 id 的 user-scope skill 覆盖 global-scope skill。
- Rule 3: 仅含 `plugin.toml` / `workflow.toml` 的目录被忽略。
- Rule 4: `load_skill` 返回展开后的 SKILL.md body，不持久化任何 active skill state。
- Rule 5: `send_message` 不接受 `selected_skill_id` 参数。
- Rule 6: 每轮 LLM 调用前，runtime 注入 SKILL.md catalog 作为 `<system-reminder>`，按 1% context window 预算 + 250 字符上限截断。
