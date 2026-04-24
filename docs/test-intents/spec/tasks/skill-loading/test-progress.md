# test-progress.md — skill 加载链路执行记录

## 状态

- 已实现测试文件：`src-tauri/tests/review_skill_loading_test.rs`
- 执行命令：`cd src-tauri && cargo test --test review_skill_loading_test -- --nocapture`
- 最近执行结果：4 passed；生产代码无需修改
- 重要说明：rules.md 中“对话开始自动注入 skill 摘要 / 调用 skill 对应工具后注入完整 SKILL.md”的描述与当前实现不完全一致。当前 lotus-app 主链路是 `SkillRegistry + SkillSessionStore + switch_skill`，`switch_skill` 返回 `skill_control` runtime patch（system_prompt/tool_defs/allowed_tools 等），不是把完整 SKILL.md 作为消息注入。因此本次按当前架构做等价可观测覆盖，后续如要严格实现 rules.md 原语义，应先修订计划/设计。

## 执行记录

| 意图 | 状态 | 备注 |
|---|---|---|
| 意图 1：skill 安装后文件正确落盘 | ✅ 通过 | 用测试安装 helper 覆盖 `.renlijia/skills/<plugin_id>/`、manifest 可读、同 id 覆盖不复制 |
| 意图 2：对话开始时 skill 摘要被注入，不包含完整内容 | ✅ 等价通过 | 当前无“自动消息注入”机制；覆盖 SkillRegistry list 暴露 name/description 且不包含 SKILL.md 正文/base prompt |
| 意图 3：LLM 调用 skill 工具后完整内容被注入 | ✅ 等价通过 | 当前通过 `switch_skill` 返回 `skill_control.system_prompt` runtime patch；覆盖 tool result 与 patch，非完整 SKILL.md 消息注入 |
| 意图 4：同一对话里 skill 摘要不重复注入 | ✅ 等价通过 | 当前通过 SkillSessionStore 维护 conversation skill state；覆盖同一 conversation resolve 两次复用同一 skill/system_prompt |

## 执行记录详情

- 2026-04-24：新增 `review_skill_loading_test.rs`，覆盖 manifest/安装目录、registry summary、switch_skill runtime patch、SkillSessionStore 同会话复用。
- 2026-04-24：运行 `cd src-tauri && cargo test --test review_skill_loading_test -- --nocapture`，结果 4 passed。仅出现既有 dead_code warning：`FILE_GEN_TOOLS`、`is_last_tool_file_generation` 未使用。
