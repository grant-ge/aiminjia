# Bug: agent 无法调用工具，提示 "tool dispatcher not configured"

## 现象

在 AIjia 对话中，当 agent 尝试调用 `bash`、`grep_content`、`list_directory` 等工具时，返回错误：

> Error: tool dispatcher not configured

agent 无法执行任何工具，只能纯文字回复。

## 期望行为

agent 能正常调用已注册的 8 个原子工具（`bash`、`read_workspace_file`、`write_file`、`edit_file`、`list_directory`、`search_files`、`get_file_info`、`grep_content`），工具执行有结果返回。

## 验收标准（用户视角）

在 AIjia 对话中输入：**"用 bash 执行 echo hello"**

agent 实际执行命令，返回 `hello`，不出现 "tool dispatcher not configured" 错误。
