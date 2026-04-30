# rules.md — memory RuntimeTool 工具层测试意图

工具层是 LLM 直接调用的接口：`write_memory` 和 `search_memory`。
测试目标：从 LLM 输入 → 工具执行 → 返回结果的完整路径行为是否符合预期。

---

## 意图 1：write_memory 成功保存记忆并返回结构化结果

**场景**
LLM 决定记住用户的一个偏好，调用 write_memory。

**前提**
- 隔离的 app_data_dir 和 workspace_path
- 工具输入包含完整字段：name、memory_type、description、content

**操作**
- 执行 write_memory 工具，传入合法输入

**断言**
- 工具执行成功，不返回错误
- 返回的 JSON 中 `status` 为 `"saved"`
- 返回的 JSON 中 `name` 与输入一致
- 返回的 JSON 中 `path` 是一个以 `.md` 结尾的相对路径
- 对应的 entry 文件真实落盘到 memory bucket 下

---

## 意图 2：write_memory 收到非法 memory_type 时返回明确错误

**场景**
LLM 传了一个不存在的 memory_type 值（如 `"custom"` 或 `"unknown"`）。

**前提**
- 工具输入中 memory_type 为非法值

**操作**
- 执行 write_memory 工具

**断言**
- 工具返回错误（ToolError）
- 错误信息包含 `"unknown memory_type"` 字样
- 错误信息列出了合法的四个值（user_preference、project_constraint、reference_info、feedback）
- 没有任何文件被写入磁盘

---

## 意图 3：write_memory 缺少必填字段时返回明确错误

**场景**
LLM 调用 write_memory 时遗漏了 name 或 content 字段。

**前提**
- 分别构造两种缺字段情况：缺 name、缺 content

**操作**
- 分别执行工具

**断言**
- 每次都返回 ToolError
- 错误信息明确指出哪个字段缺失（如 `"missing 'name'"` 或 `"missing 'content'"`）
- 没有任何文件被写入磁盘

---

## 意图 4：search_memory 根据 query 召回相关记忆并返回结构化结果

**场景**
workspace 里已存有多条记忆，LLM 用关键词搜索其中一条。

**前提**
- 先通过 write_memory 保存两条不同主题的记忆
- query 只命中其中一条

**操作**
- 执行 search_memory 工具，传入命中一条的 query

**断言**
- 工具执行成功，不返回错误
- 返回的 JSON 中 `status` 为 `"ok"`
- 返回的 JSON 中 `count` 为 `1`
- `results[0].name` 与命中的记忆一致
- `results[0].type` 与保存时的 memory_type 一致
- `results[0].content` 包含原始 content 内容
- 未命中的记忆不出现在 `results` 中

---

## 意图 5：search_memory 无命中时返回空结果而不是错误

**场景**
LLM 搜索一个与任何已存记忆都无关的词。

**前提**
- workspace 里存有若干记忆
- query 与任何 entry 都不相关

**操作**
- 执行 search_memory 工具，传入无关 query

**断言**
- 工具执行成功，不返回错误
- 返回的 JSON 中 `status` 为 `"ok"`
- 返回的 JSON 中 `count` 为 `0`
- `results` 为空数组

---

## 意图 6：write_memory 和 search_memory 使用同一个 workspace 下的同一个 bucket

**场景**
LLM 先写入一条记忆，再立刻搜索它。

**前提**
- write_memory 和 search_memory 的 deps 使用相同的 app_data_dir 和 workspace_path

**操作**
- 先执行 write_memory 保存记忆
- 再执行 search_memory 用该记忆的关键词搜索

**断言**
- search_memory 能召回刚刚写入的记忆
- `results[0].name` 与 write_memory 写入的 name 一致

---

## 意图 7：write_memory 是写操作，search_memory 是只读操作

**场景**
工具系统需要区分只读工具和写操作工具，用于权限控制。

**前提**
- 构造 write_memory 和 search_memory 工具实例

**操作**
- 分别调用 `is_read_only(input)` 方法

**断言**
- write_memory 的 `is_read_only()` 返回 `false`
- search_memory 的 `is_read_only()` 返回 `true`

---

## 意图 8：两个工具的定义名称与 TOOL_CATALOG 中的注册名一致

**场景**
工具名是 LLM 调用的标识，必须与 catalog 一致，否则 LLM 发出的调用会找不到工具。

**前提**
- 构造 write_memory 和 search_memory 工具实例
- 读取 TOOL_CATALOG

**操作**
- 分别调用 `.definition().name`
- 在 TOOL_CATALOG 中查找对应名称

**断言**
- write_memory 的 `definition().name` 为 `"write_memory"`
- search_memory 的 `definition().name` 为 `"search_memory"`
- TOOL_CATALOG 中 `"write_memory"` 存在
- TOOL_CATALOG 中 `"search_memory"` 存在
