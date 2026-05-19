# rules.md — search-tool（搜索工具）意图测试规格

## 测试范围

覆盖 AI 触发搜索工具（Bing / Bocha / Tavily 等 provider）后的端到端行为：搜索结果作为 tool_result 注入对话、provider 失败时返回错误 tool_result 而不是让 turn 崩、搜索结果在对话历史中持久可见。不包含具体 provider 的字段排序或得分计算细节。

## 待覆盖的主要场景

- 场景 1：AI 发起一次搜索调用，工具执行成功后 tool_result 含若干条结果（title / url / snippet 等可读字段）
- 场景 2：搜索 provider 返回 HTTP 错误 / 超时时，tool_result 中是结构化错误描述，turn 继续推进而不是崩溃
- 场景 3：搜索结果作为 tool_result 写入对话历史，重开对话仍能看到这段工具调用与结果
- 场景 4：搜索 query 为空 / 过长 / 含非法字符时，工具层做参数校验，返回校验错误而不是直发 provider
- 场景 5：用户在 settings 里切换 search provider，下一次搜索走新 provider
- 场景 6：provider 未配置 API key 时，工具返回明确的"未配置"错误而不是抛栈

## 待补充

> 具体意图待补全。
