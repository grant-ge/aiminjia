# 使用方式

## 常用命令

生成或更新图谱：

```bash
/understand --language zh
```

打开 dashboard。只有用户明确要求浏览器或 dashboard 时才执行：

```bash
/understand-dashboard
```

问图谱问题：

```bash
/understand-chat Runtime 权限链路怎么走？
```

分析当前改动影响面：

```bash
/understand-diff
```

解释单个文件或函数：

```bash
/understand-explain src-tauri/src/runtime/query_engine.rs
```

生成新人入门路径：

```bash
/understand-onboard
```

提取业务领域流程：

```bash
/understand-domain
```

分析 Karpathy 风格 LLM Wiki：

```bash
/understand-knowledge ~/path/to/wiki
```

## 不打开浏览器时怎么答

除非用户明确要 dashboard 或浏览器，否则用这些来源回答：

- `rg` 搜 `.understand-anything/knowledge-graph.json`
- 针对图谱的 Node/JQ 小脚本
- `docs/repo-wiki/*.md`
- 必要时读当前源码和测试确认

## 日常使用建议

问“这个模块是什么”：

1. 在图谱里搜文件或模块名。
2. 看节点 summary 和相关边。
3. 看所在 layer 和 guided tour。
4. 图谱不清楚时再读源码。

问“我的改动影响什么”：

1. 获取 changed files。
2. 匹配图谱节点。
3. 查 1-hop 上下游边。
4. 汇总影响 layer 和相关测试。
5. 明确不确定点。

## 测试问答效果

查看真实问答样例：

```bash
sed -n '1,220p' .agents/skills/userwiki/references/qa-examples.md
```

只校验问答 fixture 和 `.agents` / `.claude` 镜像：

```bash
node scripts/run-userwiki-qa-smoke.mjs --validate-only
```

列出可跑的真实问答用例：

```bash
node scripts/run-userwiki-qa-smoke.mjs --list
```

跑一个真实 CLI 问答 smoke：

```bash
node scripts/run-userwiki-qa-smoke.mjs --case settings-impact
```

如果当前环境里的 `codex` 来自 WindowsApps/AppX alias 并报 `EPERM` 或 `Access is denied`，先导出同一题 prompt：

```bash
node scripts/run-userwiki-qa-smoke.mjs --case settings-impact --prompt-out /tmp/userwiki-prompt.md
```

复核某次已生成的回答文件：

```bash
node scripts/run-userwiki-qa-smoke.mjs --case settings-impact --answer /tmp/userwiki-answer.md
```

也可以从 stdin 评分：

```bash
cat /tmp/userwiki-answer.md | node scripts/run-userwiki-qa-smoke.mjs --case settings-impact --answer -
```

`--answer -` 需要 UTF-8 stdin；Windows PowerShell 管道中文可能被转成 `?`，不确定时优先用 `--answer <path>`。

如果本机有另一个可执行的 Codex CLI，用 `USERWIKI_QA_CODEX_COMMAND` 指向它后再跑真实问答。

跑默认问答集：

```bash
node scripts/run-userwiki-qa-smoke.mjs --default --timeout-ms 180000
```

跑完整问答集。这个会启动多次真实 `codex exec`，只适合专项验收：

```bash
node scripts/run-userwiki-qa-smoke.mjs --all --timeout-ms 180000
```

注意：默认不应主动打开浏览器；脚本会用 `codex exec --sandbox read-only --ephemeral` 跑问答。
