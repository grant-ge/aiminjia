# 排障

## 没有图谱

现象：

```text
.understand-anything/knowledge-graph.json missing
```

处理：

```bash
/understand --language zh
```

## 图谱是英文

现象：节点摘要或 dashboard 文案是英文。

处理：

```bash
/understand --language zh --full
```

检查：

```bash
cat .understand-anything/config.json
```

`outputLanguage` 必须是 `zh`。

## 图谱过期

现象：当前改动文件在图谱里找不到，或 meta 指向旧 commit。

处理：

```bash
/understand
```

大范围结构变化：

```bash
/understand --language zh --full
```

## 找不到文件节点

如果当前源码文件没有图谱节点：

1. 检查 `.understand-anything/.understandignore`。
2. 确认它不是生成文件或被排除文件。
3. 跑增量 `/understand`。
4. 仍然缺失时转 `wiki-maintainer`。

## 长对话里模型忘记前文

现象：用户反馈同一个 conversation 里聊久以后，模型不记得较早内容、工具结果或上传文件线索。

先走 `docs/repo-wiki/runtime-map.md` 的 `Context Budget / Truncation Matrix`，不要只归因于模型能力。当前应按这个顺序排查：

1. 检查 `src-tauri/src/runtime/chat/history.rs` 的 `HistoryConfig::default`：默认 `max_rounds=30`、`char_budget=120_000`，生产 `load_history_via_runtime_history` 会使用它。
2. 检查本地 `<aijia_home>/users/{scope}/conversations/{conversation_id}/compact_boundaries.jsonl` 是否存在旧 boundary；重点看 `tail_message_id` 和 `summary_text`。旧 summary 质量差会让更早 transcript 只剩摘要。
3. 如果忘记的是工具输出，检查 `preprocess.rs`、`compaction.rs` 和 `tool_result_collector.rs`：tool budget、microcompact、collapse 和工具结果上限会改写旧工具结果。
4. 如果忘记的是文件、搜索或 shell 内容，检查 Read/Grep/Bash/PowerShell 的源头预算：文件默认 `1MiB`/`2000` 行，shell `512KiB`，grep `1000` 结果且跳过 `2MiB+` 文件。
5. 如果忘记的是动态上下文，检查 skill catalog、AGENTS.md、project/cognitive memory 和图片附件的独立预算。

生效性口径：

- `context_decay.apply_decay` 当前未接入普通 chat 主请求链。
- `CONTEXT_WINDOW_*` / overflow threshold 当前只做日志预警。
- `AutoCompactConfig.max_output_chars` 当前是未接入字段。
- `QueryEngine` budget/cost 配置方法当前主要由测试覆盖，普通 chat 主链未见生产阈值注入。

## Dashboard token

如果 dashboard 提示需要 access token，使用 `/understand-dashboard` 打印出来的完整 URL，包含 `?token=...`。

除非用户明确要求 dashboard 或浏览器验证，否则不要主动打开浏览器。
