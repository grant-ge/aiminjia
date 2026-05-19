# rules.md — llm-provider-routing LLM Provider 路由与降级测试意图

来源：[LUT-4](mention://issue/8fb70292-f4aa-4ec9-8c10-2ec6bcb05c76)

涉及核心模块：`llm/router.rs`（`infer_task_type` / `select_route`）、`llm/gateway.rs`（`is_retryable_error` / 重试逻辑）

---

## 意图 1：消息含薪酬关键词时 infer_task_type 返回 Analysis

**场景**
`infer_task_type` 通过最后一条 user 消息的关键词推断 TaskType。薪酬相关关键词应命中 Analysis，而不是 General。

**前提**
- 构造消息列表：`[ChatMessage::text("user", "请对薪酬公平性进行诊断")]`

**操作**
1. 调用 `infer_task_type(&messages)`

**断言**
- 返回值等于 `TaskType::Analysis`

---

## 意图 2：单独含「分析」二字的消息 infer_task_type 返回 General

**场景**
「分析」单词出现在日常对话中不应触发 Analysis——仅薪酬领域复合词才命中。

**前提**
- 构造消息列表：`[ChatMessage::text("user", "分析下伊朗最新局势")]`

**操作**
1. 调用 `infer_task_type(&messages)`

**断言**
- 返回值等于 `TaskType::General`

---

## 意图 3：消息含代码关键词时 infer_task_type 返回 CodeGen

**场景**
「python」「脚本」等关键词应命中 CodeGen。

**前提**
- 构造消息列表：`[ChatMessage::text("user", "Write a Python script to compute averages")]`

**操作**
1. 调用 `infer_task_type(&messages)`

**断言**
- 返回值等于 `TaskType::CodeGen`

---

## 意图 4：infer_task_type 使用最后一条 user 消息，忽略更早的 user 消息

**场景**
路由只看最新意图，不被历史消息干扰。

**前提**
- 构造消息列表：
  ```
  [
    ChatMessage::text("user", "请对薪酬公平性进行诊断"),
    ChatMessage::text("assistant", "好的"),
    ChatMessage::text("user", "你好"),
  ]
  ```

**操作**
1. 调用 `infer_task_type(&messages)`

**断言**
- 返回值等于 `TaskType::General`（最后一条 user 消息是「你好」，不含薪酬关键词）

---

## 意图 5：空消息列表时 infer_task_type 返回 General

**场景**
无 user 消息时安全降级为 General。

**前提**
- 构造空消息列表：`vec![]`

**操作**
1. 调用 `infer_task_type(&messages)`

**断言**
- 返回值等于 `TaskType::General`

---

## 意图 6：auto_model_routing 关闭时 select_route 始终返回 primary_model

**场景**
关闭自动路由后，无论任务类型，均使用 primary_model，不做切换。

**前提**
- 构造 `AppSettings { auto_model_routing: false, primary_model: "claude".to_string(), primary_api_key: "pk-test".to_string(), use_cloud: false, ..Default::default() }`

**操作**
1. 分别以 `TaskType::Analysis`、`TaskType::Reasoning`、`TaskType::General` 调用 `select_route(&task_type, &settings)`

**断言**
- 三次调用返回的 `route.provider` 均等于 `"claude"`
- 三次调用返回的 `route.api_key` 均等于 `"pk-test"`
- 三次调用返回的 `route.use_tools` 均为 `true`

---

## 意图 7：Analysis 任务 select_route 始终返回 primary_model 且 use_tools 为 true

**场景**
Analysis 任务需要工具调用（6 步分析工作流），即便开启了 auto_model_routing 也不能切换到无 tools 的 reasoning 模型。

**前提**
- 构造 `AppSettings { auto_model_routing: true, primary_model: "claude".to_string(), primary_api_key: "pk-test".to_string(), use_cloud: false, ..Default::default() }`

**操作**
1. 调用 `select_route(&TaskType::Analysis, &settings)`

**断言**
- `route.provider == "claude"`
- `route.api_key == "pk-test"`
- `route.use_tools == true`

---

## 意图 8：use_cloud 开启时 select_route 返回 provider 为 lotus

**场景**
云端模式下，所有任务路由到 lotus gateway，不使用本地 provider 配置。

**前提**
- 构造 `AppSettings { use_cloud: true, primary_api_key: "session-key-xyz".to_string(), cloud_model: "cloud-model-v1".to_string(), cloud_model_type: "chat".to_string(), ..Default::default() }`

**操作**
1. 调用 `select_route(&TaskType::General, &settings)`

**断言**
- `route.provider == "lotus"`
- `route.api_key == "session-key-xyz"`
- `route.model_hint == "cloud-model-v1"`
- `route.model_type == "chat"`
- `route.use_tools == true`

---

## 意图 9：use_cloud 开启且 TaskType 为 Reasoning 时 model_type 为 reasoner

**场景**
云端模式下 Reasoning 任务强制走 reasoner 端点，不受 cloud_model_type 配置影响。

**前提**
- 构造 `AppSettings { use_cloud: true, primary_api_key: "session-key-xyz".to_string(), cloud_model_type: "chat".to_string(), ..Default::default() }`

**操作**
1. 调用 `select_route(&TaskType::Reasoning, &settings)`

**断言**
- `route.provider == "lotus"`
- `route.model_type == "reasoner"`
- `route.use_tools == false`

---

## 意图 10：HTTP 429 错误被识别为 retryable

**场景**
`is_retryable_error` 对 429（Rate Limit）应返回 true，触发重试逻辑。

**前提**
- 构造 `anyhow::anyhow!("API error (429): rate limit exceeded")`

**操作**
1. 调用 `is_retryable_error(&err)`

**断言**
- 返回值为 `true`

---

## 意图 11：HTTP 401 错误不被识别为 retryable

**场景**
401 认证错误不应重试（重试没有意义），避免多余请求消耗。

**前提**
- 构造 `anyhow::anyhow!("API error (401): unauthorized")`

**操作**
1. 调用 `is_retryable_error(&err)`

**断言**
- 返回值为 `false`

---

## 意图 12：session key revoked 的 401 被识别为 auth_revoked 错误

**场景**
lotus session key 被撤销时，gateway 应触发一次 session 刷新重试，而非直接报错。

**前提**
- 构造 `anyhow::anyhow!("API error (401): session key revoked")`

**操作**
1. 调用 `is_auth_revoked_error(&err)`

**断言**
- 返回值为 `true`

---

## 意图 13：普通 401（非 session revoked）不被识别为 auth_revoked

**场景**
非 session 撤销的 401（如 tenant 禁用）不应触发 session 刷新，避免无效重试。

**前提**
- 构造 `anyhow::anyhow!("API error (401): tenant disabled")`

**操作**
1. 调用 `is_auth_revoked_error(&err)`

**断言**
- 返回值为 `false`
