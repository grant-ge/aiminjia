# Browser CLI 调研方案

> ⚠️ **本文档已被取代。** 当前真相源：
> [`docs/superpowers/specs/2026-05-09-browser-cli-externalization-design.md`](./superpowers/specs/2026-05-09-browser-cli-externalization-design.md)
> 该 spec 否决了本文档中关于 `agent-browser` 优先与 `shell+CLI` 协议的部分推荐，改为基于 `@playwright/cli` + 自包含 SKILL.md。本文档保留作决策史，请勿据此实施。

## 1. 调研目标

本调研用于回答两个问题：

1. lotus-app 是否可以用外置浏览器 CLI 替代当前内置 Playwright RuntimeTool / ConnectorEngine。
2. 哪个开源工具最适合作为 `browser-cli` skill 的默认 provider。

调研不直接改业务代码，只产出选型矩阵、POC 结果和迁移建议。

## 2. 调研结论摘要

初步结论：

- 外置 CLI 路线可行，适合降低 lotus-app 主应用复杂度。
- 默认 POC 建议优先 `@playwright/cli`，因为它最接近当前 Playwright 生态。
- 第二候选建议 `agent-browser`，因为它更贴近 AI agent 操作浏览器的 refs/snapshot 工作流。
- 真实 Chrome 登录态场景不应依赖普通隔离 Playwright 浏览器，应评估 `browser-use`、`OpenCLI` 或未来 extension/MCP 路线。
- 如果开源 CLI 无法覆盖当前高级提取能力，再考虑外置 `lotus-browser-cli`，但不应回到主 App 内置 runtime。

## 3. 已识别候选

| 候选 | 类型 | 主要用途 | 推荐级别 |
|---|---|---|---|
| `@playwright/cli` | 官方 Playwright agent CLI | 开发验证、localhost、通用浏览器自动化 | P0 |
| `agent-browser` | Agent-first 浏览器 CLI | refs/snapshot/click/fill/screenshot | P0/P1 |
| `browser-use` CLI | Browser Use CLI/daemon/profile | 更完整的 agent browser workflow，可能复用 profile | P1 |
| `OpenCLI` | Chrome bridge / 网站转 CLI | 真实 Chrome 登录态、站点适配器 | P1/P2 |
| `brow-cli` | Python Playwright CLI | 轻量候选 | P2 |
| `kuri` / `kuri-agent` | Zig/CDP/a11y CLI | 无 Node 候选 | P2 |
| 自研 `lotus-browser-cli` | 外置兼容 CLI | 统一 lotus 语义、兜底高级能力 | fallback |

## 4. 评估维度

每个候选都按以下维度评分，建议 1-5 分：

| 维度 | 说明 |
|---|---|
| 安装复杂度 | 是否需要 Node/Python/Rust/扩展/daemon；跨平台是否顺滑 |
| 命令稳定性 | 是否有清晰 CLI 文档；命令是否适合长期依赖 |
| JSON 输出 | 是否支持结构化输出，便于 agent 解析 |
| session 能力 | 是否支持保持同一浏览器 session、tab、profile |
| refs/snapshot | 是否支持 a11y/DOM snapshot refs，避免坐标点击 |
| JS 执行 | 是否支持在页面执行 JS 并返回结果 |
| 表格提取 | 是否能稳定提取 table/list/grid 数据 |
| 分页能力 | 是否支持 click-next-loop 或可由 agent 稳定编排 |
| iframe 能力 | 是否能观测和操作 iframe 内元素 |
| 截图能力 | 是否支持页面/元素截图并保存文件 |
| 下载/上传 | 是否能处理 download/upload |
| 登录态 | 是否能使用隔离 profile 或真实 Chrome profile |
| 权限安全 | 是否有危险动作约束、站点范围、只读模式等 |
| 可替换性 | 是否能只改 skill 就换 provider |
| 与当前效果一致性 | 与现有内置工具能力差距 |

## 5. 候选工具调研要点

### 5.1 `@playwright/cli`

关注点：

- 是否支持 install / skill 安装流程。
- 是否支持 open、snapshot、click、fill、screenshot、eval。
- 是否支持 headed 模式和 session 复用。
- 是否支持 JSON 或机器可读输出。
- 是否能低成本替代当前 `browse_navigate`、`read_page_content`、`page_execute_js`。

预期优势：

- 官方维护。
- 与现有 Playwright runtime 心智接近。
- 适合第一阶段 POC。

预期风险：

- 真实 Chrome 登录态和复杂业务后台能力可能不足。
- 表格提取/分页提取需要 skill recipe 或额外脚本。

### 5.2 `agent-browser`

关注点：

- 安装方式和跨平台支持。
- refs/snapshot 是否稳定。
- 是否支持 JSON 输出。
- 是否支持 session/profile。
- iframe、download、JS eval 支持情况。

预期优势：

- 更贴近 AI agent 操作浏览器的模型。
- 可能更适合长期作为 `browser-cli` skill 默认 provider。

预期风险：

- 项目成熟度和版本稳定性需验证。
- 与当前高级表格提取能力不一定等价。

### 5.3 `browser-use` CLI

关注点：

- CLI 是否独立可用，是否必须带完整 browser-use agent 框架。
- daemon/session/profile/Chrome 连接能力。
- 是否支持机器可读输出。
- 与 Claude Code / Codex skill 的适配方式。

预期优势：

- 完整 browser agent 生态。
- 可能更适合真实 profile 和持久 daemon。

预期风险：

- 相比“薄 CLI”，可能偏重。
- 命令层可能不是 lotus 想要的最小操作协议。

### 5.4 `OpenCLI`

关注点：

- Chrome Browser Bridge extension 和 local daemon 的安装复杂度。
- 是否能把已登录网站转成稳定 CLI adapter。
- 是否适合通用浏览器操作，还是更适合站点特化。
- 安全模型和站点权限边界。

预期优势：

- 真实 Chrome 登录态方向更强。
- 适合业务系统/OAuth/Cookie 场景。

预期风险：

- 不是纯 CLI，通常需要 extension/bridge。
- 更适合作为真实 Chrome 路线，不适合作为第一阶段默认替代。

### 5.5 `brow-cli`

关注点：

- 是否仍活跃维护。
- Playwright 浏览器安装和 profile/session 管理。
- CLI 命令是否稳定。
- JSON 输出和 refs 能力。

预期用途：

- 作为轻量 Python/Playwright 候选参考，不作为默认。

### 5.6 `kuri` / `kuri-agent`

关注点：

- 是否可以无 Node 运行。
- CDP 连接能力。
- a11y snapshot、click、fill、eval、screenshot 能力。
- 安装和跨平台情况。

预期用途：

- 如果团队强烈不想依赖 Node/npm，可作为备选调研。

## 6. POC 任务集

POC 必须使用同一批任务对所有候选做横向比较。

### Task A：基础导航和读取

目标：验证打开页面和读取内容。

步骤：

```bash
# 打开 URL
<provider> open "https://example.com" --json

# 获取页面 snapshot/text
<provider> snapshot --json
```

通过标准：

- 能返回当前 URL。
- 能返回标题或正文。
- 输出可被 agent 稳定解析。

### Task B：localhost 前端验证

目标：验证开发调试场景。

步骤：

```bash
<provider> open "http://localhost:3000" --json
<provider> screenshot "./artifacts/localhost.png" --json
```

通过标准：

- 能访问本地服务。
- 能保存截图。
- 如果页面报错，能读到 console 或页面错误信息。

### Task C：点击和填写

目标：验证 refs/selector 操作。

步骤：

```bash
<provider> snapshot --json
<provider> click "<ref-or-selector>" --json
<provider> fill "<ref-or-selector>" "hello@example.com" --json
```

通过标准：

- 不依赖屏幕坐标即可完成点击。
- 填写后能通过 snapshot/eval 验证值变化。

### Task D：JS 执行

目标：替代 `page_execute_js`。

步骤：

```bash
<provider> eval "document.title" --json
<provider> eval "Array.from(document.querySelectorAll('a')).slice(0,3).map(a => a.href)" --json
```

通过标准：

- 能返回 JS 结果。
- 结果可 JSON 化。

### Task E：表格提取

目标：替代 `extract_table_data`。

步骤：

```bash
<provider> eval "JSON.stringify(Array.from(document.querySelectorAll('table tr')).map(tr => Array.from(tr.cells).map(td => td.innerText)))" --json
```

通过标准：

- 能拿到 headers 和 rows。
- 能写入 workspace JSON 文件。
- 能处理无 `<table>` 但有 grid/list 的页面时给出降级策略。

### Task F：分页提取

目标：替代 `extract_with_pagination` / `browse_data`。

步骤：

```bash
<provider> eval "/* extract current page rows */" --json
<provider> click "<next-ref-or-selector>" --json
<provider> eval "/* extract next page rows */" --json
```

通过标准：

- 能判断是否存在下一页。
- 能避免重复数据。
- 能把多页数据追加到同一个 JSON 文件。

### Task G：iframe 操作

目标：验证当前 `frame_inspect` / `frame_click` 的替代能力。

步骤：

```bash
<provider> snapshot --json
<provider> click "<iframe-inner-ref-or-selector>" --json
```

通过标准：

- 能发现 iframe 内元素。
- 能点击 iframe 内按钮或链接。
- 如果 provider 不支持，记录为关键缺口。

### Task H：登录态/profile

目标：验证隔离 profile 与真实 profile。

步骤：

```bash
<provider> open "https://example-login-required.local" --profile "./.browser-profile" --json
```

通过标准：

- 隔离 profile 能保持登录态。
- 真实 Chrome profile 场景有明确 provider 或替代路线。

### Task I：下载/上传

目标：验证文件动作。

步骤：

```bash
<provider> click "<download-ref>" --json
<provider> upload "<file-input-ref>" "./fixtures/sample.txt" --json
```

通过标准：

- 下载文件路径可控。
- 上传前必须经过用户确认。

## 7. 评分表模板

调研时为每个 provider 填写：

| 维度 | 分数 | 证据 | 备注 |
|---|---:|---|---|
| 安装复杂度 |  |  |  |
| 命令稳定性 |  |  |  |
| JSON 输出 |  |  |  |
| session 能力 |  |  |  |
| refs/snapshot |  |  |  |
| JS 执行 |  |  |  |
| 表格提取 |  |  |  |
| 分页能力 |  |  |  |
| iframe 能力 |  |  |  |
| 截图能力 |  |  |  |
| 下载/上传 |  |  |  |
| 登录态 |  |  |  |
| 权限安全 |  |  |  |
| 可替换性 |  |  |  |
| 与当前效果一致性 |  |  |  |

## 8. 决策规则

默认 provider 必须满足：

- 基础导航、读取、截图、点击、填写全部通过。
- JS 执行可用，或者有稳定替代方案。
- 输出足够机器可读，agent 可以可靠解析。
- 安装过程可文档化，不要求 lotus-app 打包浏览器 runtime。
- 失败时能通过 skill 给出明确恢复动作。

如果两个 provider 都满足，则优先级：

1. 官方稳定性。
2. agent 操作体验。
3. 与当前能力一致性。
4. 安装体积和复杂度。
5. 真实 Chrome/profile 扩展能力。

## 9. 预期产出

调研完成后应产出：

- `docs/2026-05-09-browser-cli-provider-poc.md`
- `docs/2026-05-09-browser-cli-parity-report.md`
- `docs/2026-05-09-browser-cli-provider-decision.md`
- `browser-cli` skill 草案或最终版。

## 10. 当前建议

当前无需等全部调研完成再行动。建议先做两条 POC：

1. `@playwright/cli`：验证能否最快替代当前内置 Playwright 工具。
2. `agent-browser`：验证是否更适合作为长期 agent-first CLI。

如果二者都不能稳定覆盖表格、分页、iframe 三个关键场景，再评估外置 `lotus-browser-cli`。

## 11. 外部参考

- `agent-browser`：https://github.com/vercel-labs/agent-browser
- Playwright Agent CLI：https://playwright.dev/agent-cli/introduction
- Playwright CLI GitHub：https://github.com/microsoft/playwright-cli
- Browser Use CLI：https://docs.browser-use.com/open-source/browser-use-cli
- Browser Use GitHub：https://github.com/browser-use/browser-use
- OpenCLI：https://github.com/jackwener/OpenCLI
- brow-cli：https://pypi.org/project/brow-cli/
- Kuri：https://github.com/justrach/kuri
