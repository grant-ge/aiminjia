# rules.md — subagent 工具白名单与内置 Agent 工具集测试意图

这组意图验证两件事：父 LLM 看到的工具白名单，和真实执行时允许的工具白名单，必须是同一份；同时，内置 `explore` agent 不能因为旧别名而把自己过滤空。

---

## 意图 1：空 allowed_tools 的子代理在 schema 暴露与执行放行上必须使用同一份最终白名单

**场景**
用户启动 `general-purpose` 子代理时，AgentDefinition 的 `allowed_tools = []`，系统承诺这代表“先取可用工具全集，再套系统级禁用和递归保护”，而不是“禁止所有工具”。父 LLM 即使手写一个 schema 看不到的递归 `spawn_subagent` tool call，也不能把它执行成功。

**前提**
- 子代理类型为 `general-purpose`
- `subagent_type = "general-purpose"`
- `prompt = "请总结当前目录中的项目结构"`
- `description = "summary"`
- AgentDefinition 的 `allowed_tools = []`
- AgentDefinition 的 `disallowed_tools = []`
- 可用工具集合里至少包含 `read_workspace_file`、`write_file`、`spawn_subagent`、`task_output`、`ask_user_question`
- 这轮 LLM 模拟返回一个手写工具调用：
  - `tool_name = "spawn_subagent"`
  - `arguments.subagent_type = "general-purpose"`
  - `arguments.prompt = "再开一个子代理"`
  - `arguments.description = "recursive spawn"`
  - `arguments.run_in_background = false`

**操作**
1. 启动 `general-purpose` 子代理，读取它最终暴露给 LLM 的 `tool_defs`
2. 让 LLM 返回上面的手写 `spawn_subagent` tool call
3. 执行该 tool call

**断言**
- `tool_defs` 中包含 `read_workspace_file`
- `tool_defs` 中包含 `write_file`
- `tool_defs` 中包含 `task_output`
- `tool_defs` 中不包含 `ask_user_question`
- `tool_defs` 中不包含 `spawn_subagent`
- 手写 `spawn_subagent` tool call 返回 `tool_result.is_error == true`
- 手写 `spawn_subagent` tool call 的 `tool_result.content` 包含字符串 `"spawn_subagent"`
- 手写 `spawn_subagent` tool call 的 `tool_result.content` 包含字符串 `"not allowed"` 或 `"disabled"`

---

## 意图 2：`explore` 内置 agent 的允许工具必须使用 catalog canonical 名，而不是旧别名

**场景**
用户选择 `explore` 内置 agent 时，它承诺是只读探索 agent，应该能读工作区、grep、搜文件、列目录和 web search。系统不能把 `read_file`、`grep`、`glob` 这些旧别名拿来冒充真实工具名，否则 resolve 后会被过滤掉。

**前提**
- 子代理类型为 `explore`
- `subagent_type = "explore"`
- `prompt = "找出 Cargo.toml 里定义的 package 名称"`
- `description = "explore"`
- 这轮 LLM 仅允许使用内置工具，不额外注入自定义工具

**操作**
1. 启动 `explore` 子代理
2. 读取它最终暴露给 LLM 的 `tool_defs`

**断言**
- `tool_defs` 中正好包含这 5 个 canonical 名称：
  - `read_workspace_file`
  - `grep_content`
  - `search_files`
  - `list_directory`
  - `web_search`
- `tool_defs` 中不包含 `read_file`
- `tool_defs` 中不包含 `grep`
- `tool_defs` 中不包含 `glob`
- `tool_defs.len() == 5`

