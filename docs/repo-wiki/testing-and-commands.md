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

## UserWiki QA Smoke

只校验问答案例结构和 `.agents` / `.claude` 镜像：

```bash
node scripts/run-userwiki-qa-smoke.mjs --validate-only
```

列出可验收的问题：

```bash
node scripts/run-userwiki-qa-smoke.mjs --list
```

有可执行 Codex CLI 时，跑真实只读问答：

```bash
node scripts/run-userwiki-qa-smoke.mjs --case auth-billing-user-scope-impact --timeout-ms 180000
```

如果当前环境里的 `codex` 是 WindowsApps/AppX alias 并报 `EPERM` 或 `Access is denied`，先导出同一题 prompt，再评分回答文件。PowerShell 示例：

```powershell
node scripts/run-userwiki-qa-smoke.mjs --case auth-billing-user-scope-impact --prompt-out $env:TEMP\userwiki-prompt.md
node scripts/run-userwiki-qa-smoke.mjs --case auth-billing-user-scope-impact --answer $env:TEMP\userwiki-answer.md
```

也可以从 stdin 评分，适合把另一个 Codex surface 的回答直接管道传入。PowerShell 需要先固定 UTF-8 输出编码：

```powershell
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
Get-Content -Raw -Encoding UTF8 $env:TEMP\userwiki-answer.md | node scripts/run-userwiki-qa-smoke.mjs --case auth-billing-user-scope-impact --answer -
```

`--answer -` 需要 UTF-8 stdin；Windows PowerShell 管道中文可能被转成 `?`，不确定时优先用 `--answer <path>`。在 cmd.exe 里把 `$env:TEMP` 换成 `%TEMP%`。

如果本机有另一个可执行的 Codex CLI，用 `USERWIKI_QA_CODEX_COMMAND` 指向它后再跑真实问答。

## Frontend Checks

常用命令：

```bash
pnpm test
pnpm build
```

聚焦文件改动时优先跑相关 Vitest 文件。涉及 Tauri dev server 时，`pnpm tauri:dev` 通过 `scripts/tauri-dev.mjs` 启动，不再准备安装包内置 runtime 资源。

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

`test-intents-aijia-cli` 增强补充了测试体系链路：

- `package.json` 定义 `dev:with-pilot`、`build:with-pilot` 和 `ensure:e2e-prereq` 等 E2E/pilot scripts。
- `scripts/ensure-e2e-prereq.sh` 检查 cargo git-fetch-with-cli 和 ssh-agent；`dev:with-pilot` 通过 `scripts/tauri-dev.mjs --features e2e` 启动。
- `.agents/skills/usertest-intents/SKILL.md` 是用户级入口，负责解释 AEIT 和路由跑/写意图。
- `.agents/skills/test-intents-cli-author/SKILL.md` 约束 `aijia <verb>` 子命令必须封装原子 UI 操作，复杂流程由 rules.md 串联。
- `docs/test-intents/cli-gap.md` 是 CLI 缺口清单；`tauri-pilot aijia` 的实现不在当前仓库，实际可用命令以 sibling `tauri-pilot` 仓库和 PATH 上安装版本为准。

已知缺口：`docs/test-intents/README.md` 和 `usertest-intents` skill 仍写 13 个 task，但当前 `docs/test-intents/spec/tasks/*/rules.md` 有 14 个目录；维护 rules 或回答 task 覆盖时要先说明这个口径漂移。
