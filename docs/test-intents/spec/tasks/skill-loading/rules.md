# rules.md — skill 加载链路测试意图

## 意图 1：skill 安装后文件正确落盘

**前提**
- 本地有一个合法的 skill 目录，包含 `plugin.toml` 或 `SKILL.md`，有 `plugin.id` 字段

**操作**
- 执行 skill 安装

**断言**
- `.renlijia/skills/<plugin_id>/` 目录存在
- 目录内可读取到 manifest，`plugin.id` 与安装源一致
- 安装同一个 plugin_id 第二次，不会产生两个目录，只有一个（覆盖）

---

## 意图 2：对话开始时 skill 摘要被注入，不包含完整内容

**前提**
- skill 已安装在 `.renlijia/skills/` 下

**操作**
- 发第一条消息，触发一次对话

**断言**
- LLM 收到的上下文里包含该 skill 的 name 和 description
- 不包含完整 SKILL.md 正文内容
- 摘要以 system-reminder 形式注入（不是 system prompt）

---

## 意图 3：LLM 调用 skill 工具后完整内容被注入

**前提**
- skill 摘要已注入到上下文

**操作**
- LLM 调用该 skill 对应的工具

**断言**
- 完整 SKILL.md 内容出现在后续消息中
- tool_result 返回 "Launching skill: <skill_name>"
- 完整内容只注入一次，不重复

---

## 意图 4：同一对话里 skill 摘要不重复注入

**前提**
- skill 摘要已在本次对话发送过一次

**操作**
- 同一个对话继续发第二条、第三条消息

**断言**
- skill 摘要在整个对话历史里只出现一次
- 不会随每条消息重复追加
