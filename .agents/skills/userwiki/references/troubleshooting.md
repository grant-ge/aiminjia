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

## Dashboard token

如果 dashboard 提示需要 access token，使用 `/understand-dashboard` 打印出来的完整 URL，包含 `?token=...`。

除非用户明确要求 dashboard 或浏览器验证，否则不要主动打开浏览器。
