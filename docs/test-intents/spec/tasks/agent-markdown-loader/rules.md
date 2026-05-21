# rules.md — 用户自定义 agent Markdown 加载测试意图

用户通过 `~/.renlijia/users/<scope>/agents/` 放进来的 `.md` 文件，是最直接的自定义 agent 入口。这个入口必须 fail-closed：目录要先存在，文本字段要先 trim，再决定能不能注册。

---

## 意图 1：激活用户 scope 后，`users/<scope>/agents/` 目录必须存在

**场景**
新用户第一次登录后，系统承诺会准备好 agents 目录，让用户能放自己的 `.md` agent 文件；如果目录不存在，loader 只能跳过，用户会误以为自己的 agent 丢了。

**前提**
- 用户 scope 是 `t_1__u_2`
- 该 scope 尚未被激活，本地 agents 目录不存在

**操作**
- 用户 scope `t_1__u_2` 激活，系统准备用户工作目录

**验收标准**
- `users/t_1__u_2/agents/` 目录存在
- 这个目录是一个真实目录，不是普通文件
- 目录创建后，后续把 `.md` 文件放进去就能被系统加载器找到

---

## 意图 2：`name` 只要 trim 后为空，就必须拒绝注册

**场景**
用户写了一个名字看起来像有值、但 trim 后为空的 agent 文件。系统不能把它注册成一个空名字 agent，否则后续覆盖和查找都会乱掉。

**前提**
- 用户在 `users/t_1__u_2/agents/` 目录下放置了一个名为 `empty-name.md` 的 agent 文件
- 该文件的 name 字段为纯空白字符（trim 后为空），description 字段为 `"有效描述"`，allowed_tools 包含 `read_workspace_file`，body 内容为 `You are a helper.`

**操作**
- 用户 scope 激活后系统自动加载 agents 目录，扫描并解析该文件

**验收标准**
- 系统拒绝注册该 agent，加载结果为错误
- 错误信息包含文件路径 `empty-name.md`
- registry 中不存在名字为空字符串的 agent
- registry 中也不存在被空白名字污染的条目

---

## 意图 3：`description` 或 body 只要 trim 后为空，就必须拒绝注册

**场景**
用户写的 agent 不能只有壳子。描述和 system prompt 都是 agent 的产品承诺，trim 后为空就不算有效 agent。

**前提**
- 用户在 `users/t_1__u_2/agents/` 目录下放置了一个名为 `blank-body.md` 的 agent 文件
- 该文件的 name 字段为 `blank-body`，description 字段为纯空白字符（trim 后为空），allowed_tools 包含 `read_workspace_file`，body 内容为纯空白字符

**操作**
- 用户 scope 激活后系统自动加载 agents 目录，扫描并解析该文件

**验收标准**
- 系统拒绝注册该 agent，加载结果为错误
- 错误信息包含文件路径 `blank-body.md`
- registry 中不存在名为 `blank-body` 的 agent

---

## 意图 4：allowed_tools 里的工具名只要 trim 后为空，就必须拒绝注册

**场景**
用户可以限制自己的 agent 只能用某些工具，但这些工具名必须是真正的工具名，不能是空白字符串或带空格的伪名。

**前提**
- 用户在 `users/t_1__u_2/agents/` 目录下放置了一个名为 `blank-tool.md` 的 agent 文件
- 该文件的 name 字段为 `blank-tool`，description 为有效描述，allowed_tools 列表中有一项为纯空白字符串（trim 后为空），同时还有一项带前后空格的 ` read_workspace_file `，body 内容为 `You are a helper.`

**操作**
- 用户 scope 激活后系统自动加载 agents 目录，扫描并解析该文件

**验收标准**
- 系统拒绝注册该 agent，加载结果为错误
- 错误信息包含文件路径 `blank-tool.md`
- registry 中不存在名为 `blank-tool` 的 agent
- `allowed_tools` 不会把 `" read_workspace_file "` 当成合法工具名静默接受

