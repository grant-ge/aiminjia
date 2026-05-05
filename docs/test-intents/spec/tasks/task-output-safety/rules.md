# rules.md — task_output 安全性测试意图

`task_output` 是父代理读取后台子代理 transcript 的入口。它必须只读自己那条 transcript，不能把路径片段当成文件路径的一部分。

---

## 意图 1：合法 task_id 只能读取自己的 transcript，并按 offset 返回尾部行

**场景**
后台子代理已经写入自己的 JSONL transcript，父代理用精确的 `task_id` 和 `offset` 读取增量内容时，必须拿到自己的那份 transcript，而不是其他任务的文件。

**前提**
- 用户 scope 解析后得到的 transcript 目录是 `users/t_test__u_test/subagent_transcripts`
- 文件 `users/t_test__u_test/subagent_transcripts/agent-3f9a.jsonl` 内容依次为：
  1. `{"role":"assistant","content":"line-1"}`
  2. `{"role":"tool","content":"line-2"}`
  3. `{"role":"assistant","content":"line-3"}`
- 同目录下还存在另一个文件 `users/t_test__u_test/subagent_transcripts/agent-evil.jsonl`，内容为：
  1. `{"role":"assistant","content":"evil-1"}`

**操作**
- 调用 `task_output(task_id="agent-3f9a", offset=1)`

**断言**
- 返回 `Ok`
- 返回体 `lines` 等于下面两个字符串组成的数组，顺序不变：
  1. `{"role":"tool","content":"line-2"}`
  2. `{"role":"assistant","content":"line-3"}`
- 返回体 `new_offset == 3`
- 返回体 `lines` 不包含 `evil-1`
- 返回体 `lines` 不包含 `agent-evil.jsonl` 中的任何内容

---

## 意图 2：路径片段 task_id 必须被拒绝

**场景**
模型传入的 `task_id` 不能包含路径语义。只要它试图表达上级目录、子目录、隐藏文件或反斜杠路径，工具都必须拒绝，而不是去拼文件路径。

**前提**
- 用户 scope 解析后得到的 transcript 目录是 `users/t_test__u_test/subagent_transcripts`
- 目录中存在一个合法文件 `users/t_test__u_test/subagent_transcripts/agent-3f9a.jsonl`
- 下面 4 个输入分别作为 `task_id` 传入：
  1. `../escape`
  2. `a/b`
  3. `a\\b`
  4. `.hidden`

**操作**
- 对每个 `task_id` 分别调用一次 `task_output(task_id=<input>, offset=0)`

**断言**
- 四个输入都返回 `Err`
- 四个错误都包含字符串 `"invalid task_id"`
- 四个错误都不包含任何 transcript 行内容
- 四个错误都不读取 `users/t_test__u_test/subagent_transcripts/agent-3f9a.jsonl`

