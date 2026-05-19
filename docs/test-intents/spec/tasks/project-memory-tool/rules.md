# rules.md — project-memory-tool（记忆工具层 / 产品行为）意图测试规格

## 测试范围

覆盖 AI 调用 `write_memory` / `search_memory` 工具时面向用户的可观察行为：工具执行后文件实际落盘、检索能返回相关记忆内容、下一轮对话能在 system prompt 中看到刚写入的记忆。不包含 ProjectMemoryService 内部 recall 评分 / index 重建细节（那归 `memory-service` 测试集）。

## 待覆盖的主要场景

- 场景 1：AI 调用 `write_memory` 写入一条记忆后，对应 entry 文件在 workspace 下出现，frontmatter 完整
- 场景 2：AI 调用 `search_memory` 用关键词检索，tool_result 中含命中的记忆 name + content 片段
- 场景 3：刚写入的记忆在紧跟的下一轮 turn 的 system prompt 里出现（注入链路打通）
- 场景 4：`search_memory` 无命中时返回结构化"无结果"提示，不是空字符串、不抛错
- 场景 5：`write_memory` 参数缺失或非法 memory_type 时，tool_result 返回校验错误，不留下半成品文件
- 场景 6：连续两次 `write_memory` 同名 entry，第二次更新内容，文件数量不增加

## 待补充

> 具体意图待补全。
