# Testing And Commands

## RepoWiki Validation

```bash
node scripts/check-repowiki.mjs
```

该命令检查：

- `docs/repo-wiki/` 必需页面存在。
- `docs/README.md` 已挂 `repo-wiki/` 入口。
- `.understand-anything/config.json` 使用中文。
- `.understand-anything/knowledge-graph.json` 有 nodes、edges、layers、tour。
- RepoWiki 内部本地链接指向存在的文件。

## Understand-Anything Graph Validation

```bash
node --input-type=module -e "import fs from 'node:fs'; import { validateGraph } from '/Users/a20250311/github/Understand-Anything/understand-anything-plugin/packages/core/dist/schema.js'; const graph=JSON.parse(fs.readFileSync('.understand-anything/knowledge-graph.json','utf8')); const result=validateGraph(graph); const bad=result.issues.filter(i=>i.level==='fatal'||i.level==='dropped'); console.log(JSON.stringify({issues:result.issues.length,bad:bad.length}, null, 2)); process.exit(bad.length ? 1 : 0);"
```

通过标准：`bad` 为 `0`。

## Frontend Checks

常用命令：

```bash
pnpm test
pnpm build
```

聚焦文件改动时优先跑相关 Vitest 文件。涉及 Tauri dev server 时，`pnpm tauri:dev` 会先执行 `pnpm ensure:runtime`。

## Rust Checks

常用命令：

```bash
cd src-tauri && cargo check
cd src-tauri && cargo test --test review_tauri_event_adapter_test
cd src-tauri && cargo test --test runtime_dependencies_manager_test
```

`review_*.rs` 是架构护栏，不要把它们当普通实现测试降权。

## Release Checks

发布以 `docs/release-playbook.md` 为准。关键路径：

- `scripts/release.py`: 顺序守卫。
- `.github/workflows/build-desktop.yml`: Windows unsigned staging。
- `scripts/build-and-sign-macos.sh`: macOS 本地签名、公证、上传。
- `scripts/release-windows.ps1`: Windows 本地签名上传。
- `.github/workflows/finalize-release.yml`: 生成 `update.json`，自动更新真正生效。

## Intent Tests

AEIT/test-intents 是真实账号 L4 验收，不等同于 CI 单测。

入口：

- `docs/test-intents/README.md`
- `.agents/skills/usertest-intents/SKILL.md`
- `docs/test-intents/spec/tasks/<task>/rules.md`

跑意图或写意图时必须走对应 skill，不直接凭经验改 rules。
