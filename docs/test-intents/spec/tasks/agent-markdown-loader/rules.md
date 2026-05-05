# rules.md — 用户自定义 agent Markdown 加载测试意图

用户通过 `~/.renlijia/users/<scope>/agents/` 放进来的 `.md` 文件，是最直接的自定义 agent 入口。这个入口必须 fail-closed：目录要先存在，文本字段要先 trim，再决定能不能注册。

---

## 意图 1：激活用户 scope 后，`users/<scope>/agents/` 目录必须存在

**场景**
新用户第一次登录后，系统承诺会准备好 agents 目录，让用户能放自己的 `.md` agent 文件；如果目录不存在，loader 只能跳过，用户会误以为自己的 agent 丢了。

**前提**
- 用户 scope 是 `t_1__u_2`
- root 目录是一个空的 TempDir
- 调用 user scope 激活流程后，`UserScopedPaths::agents_dir()` 应指向 `users/t_1__u_2/agents`

**操作**
1. 激活 `t_1__u_2` 这个用户 scope
2. 读取 `agents_dir()` 对应的路径

**断言**
- `users/t_1__u_2/agents/` 目录存在
- 这个目录是一个真实目录，不是普通文件
- 目录创建后，后续把 `.md` 文件放进去就能被 registry loader 找到

---

## 意图 2：`name` 只要 trim 后为空，就必须拒绝注册

**场景**
用户写了一个名字看起来像有值、但 trim 后为空的 agent 文件。系统不能把它注册成一个空名字 agent，否则后续覆盖和查找都会乱掉。

**前提**
- 文件路径是 `users/t_1__u_2/agents/empty-name.md`
- frontmatter 内容为：
  ```yaml
  ---
  name: "   "
  description: "有效描述"
  allowed_tools: ["read_workspace_file"]
  ---
  ```
- body 内容是：
  `You are a helper.`

**操作**
- 调用 registry loader 读取该文件

**断言**
- loader 返回 `Err`
- 错误信息包含文件路径 `empty-name.md`
- registry 中不存在名字为空字符串的 agent
- registry 中也不存在被空白名字污染的条目

---

## 意图 3：`description` 或 body 只要 trim 后为空，就必须拒绝注册

**场景**
用户写的 agent 不能只有壳子。描述和 system prompt 都是 agent 的产品承诺，trim 后为空就不算有效 agent。

**前提**
- 文件路径是 `users/t_1__u_2/agents/blank-body.md`
- frontmatter 内容为：
  ```yaml
  ---
  name: "blank-body"
  description: "   "
  allowed_tools: ["read_workspace_file"]
  ---
  ```
- body 内容只有空白字符：
  `"   "`

**操作**
- 调用 registry loader 读取该文件

**断言**
- loader 返回 `Err`
- 错误信息包含文件路径 `blank-body.md`
- registry 中不存在名为 `blank-body` 的 agent

---

## 意图 4：allowed_tools 里的工具名只要 trim 后为空，就必须拒绝注册

**场景**
用户可以限制自己的 agent 只能用某些工具，但这些工具名必须是真正的工具名，不能是空白字符串或带空格的伪名。

**前提**
- 文件路径是 `users/t_1__u_2/agents/blank-tool.md`
- frontmatter 内容为：
  ```yaml
  ---
  name: "blank-tool"
  description: "有效描述"
  allowed_tools: [" read_workspace_file ", "   "]
  ---
  ```
- body 内容为：
  `You are a helper.`

**操作**
- 调用 registry loader 读取该文件

**断言**
- loader 返回 `Err`
- 错误信息包含文件路径 `blank-tool.md`
- registry 中不存在名为 `blank-tool` 的 agent
- `allowed_tools` 不会把 `" read_workspace_file "` 当成合法工具名静默接受

