# 安装和初始化

## UserWiki 是什么

在 lotus-app 里，UserWiki 是这几部分组成的：

- Understand-Anything 图谱：`.understand-anything/knowledge-graph.json`
- RepoWiki 文档：`docs/repo-wiki/`
- 用户入口 skill：`userwiki`
- 维护入口 skill：`wiki-maintainer`

## 安装 Understand-Anything

标准安装走 Understand-Anything 官方 one-line installer：

```bash
curl -fsSL https://raw.githubusercontent.com/Lum1104/Understand-Anything/main/install.sh | bash
```

Codex 场景如果要跳过平台选择提示，直接指定 `codex`：

```bash
curl -fsSL https://raw.githubusercontent.com/Lum1104/Understand-Anything/main/install.sh | bash -s codex
```

安装器会把仓库 clone 到：

```text
~/.understand-anything/repo
```

并为所选平台创建对应 symlink。安装后重启 Codex 会话，让新 skill 被发现。

如果需要彻底重装，先删除旧安装目录，再跑官方安装脚本：

```bash
rm -rf ~/.understand-anything/repo
curl -fsSL https://raw.githubusercontent.com/Lum1104/Understand-Anything/main/install.sh | bash -s codex
```

本机也可能有开发/备份 clone，例如：

```text
/Users/a20250311/github/Understand-Anything
```

但标准安装和团队说明应以 `~/.understand-anything/repo` 的官方 installer 路径为准。

## 初始化项目图谱

中文项目必须用中文输出：

```bash
/understand --language zh
```

大型项目建议先跑子目录，再跑全库：

```bash
/understand src-tauri/src/runtime
/understand --language zh --full
```

## 校验初始化结果

回答安装或中文初始化问题时，必须给出本项目的校验命令：

```bash
node scripts/check-repowiki.mjs
node scripts/run-userwiki-qa-smoke.mjs --validate-only
```

如果要确认图谱 schema，再跑：

```bash
node --input-type=module -e "import fs from 'node:fs'; import { validateGraph } from '/Users/a20250311/github/Understand-Anything/understand-anything-plugin/packages/core/dist/schema.js'; const graph=JSON.parse(fs.readFileSync('.understand-anything/knowledge-graph.json','utf8')); const result=validateGraph(graph); const bad=result.issues.filter(i=>i.level==='fatal'||i.level==='dropped'); console.log(JSON.stringify({success:result.success,issues:result.issues.length,bad:bad.length,nodes:graph.nodes.length,edges:graph.edges.length,layers:graph.layers?.length??0,tour:graph.tour?.length??0}, null, 2)); process.exit(bad.length ? 1 : 0);"
```

## 团队提交策略

建议提交：

- `.understand-anything/.understandignore`
- `.understand-anything/config.json`
- `.understand-anything/knowledge-graph.json`
- `.understand-anything/enhancements/*.json`
- `.understand-anything/fingerprints.json`
- `.understand-anything/meta.json`
- `docs/repo-wiki/**`
- `.agents/skills/userwiki/**`
- `.agents/skills/wiki-maintainer/**`
- `.claude/skills/userwiki/**`
- `.claude/skills/wiki-maintainer/**`
- `scripts/check-repowiki.mjs`
- `scripts/apply-understand-enhancements.mjs`

不建议提交：

- `.understand-anything/intermediate/`
- `.understand-anything/tmp/`
- `.understand-anything/diff-overlay.json`

## 保持更新

普通代码改动后跑增量：

```bash
/understand
```

大范围结构变化后跑全量：

```bash
/understand --language zh --full
```

如果要补 enhancement、RepoWiki 页面或校验规则，切到 `wiki-maintainer`。
