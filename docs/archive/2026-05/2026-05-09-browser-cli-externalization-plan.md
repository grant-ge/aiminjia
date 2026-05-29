# Browser CLI 外置化架构方案

> ⚠️ **本文档已被取代。** 当前真相源：
> [`docs/superpowers/specs/2026-05-09-browser-cli-externalization-design.md`](./superpowers/specs/2026-05-09-browser-cli-externalization-design.md)
> 该 spec 否决了本文档中关于 `agent-browser` 优先与 `shell+CLI` 协议的部分推荐，改为基于 `@playwright/cli` + 自包含 SKILL.md。本文档保留作决策史，请勿据此实施。

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 lotus-app 当前内置的 Playwright 浏览器 runtime 从主应用中剥离，改为由 agent 通过 `browser-cli` skill 调用外置浏览器 CLI 完成网页打开、读取、点击、填写、截图和数据提取。

**Architecture:** lotus-app 不再把浏览器作为一等内置 runtime 管理；主应用只保留 shell、文件、权限、skill、日志/审计等通用能力。浏览器能力由 skill 描述操作协议，由外部 CLI 负责 session、profile、CDP/Playwright、浏览器进程、截图和 JSON 输出。

**Tech Stack:** Codex/Lotus skill、shell tool、外置浏览器 CLI（优先 `@playwright/cli` 或 `agent-browser`）、可选 `browser-use` / `OpenCLI`、现有 Rust/Tauri 工具注册体系作为迁移对象。

---

## 1. 背景

当前 lotus-app 的浏览器能力走内置链路：

```mermaid
flowchart LR
  A["LLM tool call"] --> B["RuntimeTool"]
  B --> C["ConnectorEngine"]
  C --> D["PlaywrightBrowser"]
  D --> E["Node sidecar browser.js"]
  E --> F["Playwright Chromium profile"]
```

这条链路能工作，但主应用承担了大量浏览器 runtime 责任：

- 打包或寻找 Node.js。
- 打包或寻找 Playwright browser binaries。
- 管理 `playwright-runtime/browser.js` sidecar。
- 维护 stdin/stdout JSON line 协议。
- 管理浏览器 profile、重启、截图、下载、iframe 操作。
- 在 Rust 里维护 `ConnectorEngine` 和一组浏览器 `RuntimeTool`。

典型代码位置：

- `src-tauri/src/connector/playwright_browser.rs`：`PlaywrightBrowser` 管理 Node sidecar、浏览器进程、profile、命令协议。
- `src-tauri/src/connector/engine.rs`：`ConnectorEngine` 暴露 `browser_navigate`、`browser_read_content`、`browser_execute_js`、`browser_extract_table_data`、`browser_frame_click` 等方法。
- `src-tauri/playwright-runtime/browser.js`：Playwright 侧真正执行浏览器操作。
- `src-tauri/src/runtime/tools/builtin/browser.rs`：浏览器 RuntimeTool 包装。
- `src-tauri/src/runtime/tools/catalog.rs`：浏览器工具 schema。
- `scripts/setup-playwright.sh` / `scripts/setup-playwright.ps1`：安装内置 Playwright runtime。

用户当前目标是降低主应用复杂度，把浏览器 runtime 从 lotus-app 中剥离出去。这个目标与“继续补齐 Browser RuntimeTool / ConnectorEngine”方向相反。

## 2. 对标 claude-code-best 的判断

`claude-code-best` 的浏览器相关能力不是把 Playwright 作为核心内置 runtime，而是分成几类外置/动态能力：

1. Chrome 真实浏览器能力走 `claude-in-chrome` MCP。
   - CLI 侧通过 `setupClaudeInChrome()` 注入动态 MCP config、allowed tools、system prompt。
   - Chrome extension / Native Host / MCP server 共同完成真实 Chrome tab 操作。
   - 工具形态是 `mcp__claude-in-chrome__*`，不是内置 Playwright RuntimeTool。

2. Computer Use 是独立 MCP 能力。
   - 负责桌面截图、鼠标键盘、窗口等通用 GUI 能力。
   - 对浏览器类任务会提示优先使用 Chrome MCP，而不是把浏览器任务降级成纯坐标点击。

3. Playwright 更多是 verifier / 插件 / MCP 生态能力。
   - 适合开发验证、localhost、前端回归、CI。
   - 不作为主 CLI/App 的内置浏览器 runtime 核心。

对 lotus-app 的启示：

- 如果目标是操作真实用户 Chrome，最终更像 Chrome extension + MCP，而不是 Playwright sidecar。
- 如果目标是开发验证和普通网页自动化，外置 CLI/Playwright CLI 比内置 runtime 更轻。
- 如果目标是降低主应用复杂度，应把浏览器能力变成 skill/plugin/CLI/MCP 这类可替换能力，而不是继续加深主 App 的 `browser` scope。

## 3. 目标与非目标

### 3.1 目标

- 主应用不再打包 Node / Playwright / Chromium 作为浏览器 runtime。
- 主应用不再维护专门的 `Browser RuntimeTool / ConnectorEngine / PlaywrightBrowser` 主路径。
- agent 通过 shell 调用外置浏览器 CLI。
- 浏览器操作流程由 `browser-cli` skill 描述。
- 保持用户可感知能力基本一致：打开网页、读取内容、点击、填写、截图、执行 JS、提取表格、分页提取、localhost 调试。
- 迁移过程可回滚：先新增 skill 和 POC，再隐藏内置工具，最后删除旧 runtime。

### 3.2 非目标

- 不追求保留当前 Rust 内部工具形态。
- 不在 lotus-app 内新建另一个 `BrowserRuntime` 或 `ConnectorEngine` 替代层。
- 不在主 App 里解析每一种浏览器 CLI 的私有协议。
- 不在第一阶段实现 Chrome extension / Native Messaging。
- 不在第一阶段实现 Computer Use。

## 4. 推荐架构

目标架构：

```mermaid
flowchart LR
  A["lotus agent"] --> B["shell tool"]
  B --> C["browser-cli skill"]
  C --> D["external browser CLI"]
  D --> E["browser session/profile"]
  D --> F["JSON/text/screenshot artifacts"]
  F --> A
```

边界划分：

| 层级 | 责任 | 不负责 |
|---|---|---|
| lotus-app | shell、文件、权限审批、skill 加载、日志审计 | 浏览器进程、Playwright、Chromium、profile、CDP 协议 |
| browser-cli skill | 安装检查、provider 选择、命令协议、失败恢复、安全规则 | 维护 Rust 工具 schema、长期进程管理 |
| external CLI | 浏览器 session、导航、snapshot、点击、输入、截图、JS 执行 | lotus 对话状态、权限 UI、业务语义 |
| agent | 根据任务拆步骤，读取 CLI 输出，决定下一步 | 直接依赖某个 App 内部 connector |

## 5. Provider 策略

### 5.1 默认 provider：`@playwright/cli`

定位：保守替代当前 Playwright sidecar。

优点：

- 官方维护，稳定性更高。
- 与当前实现同属 Playwright 生态，迁移风险较低。
- 适合 localhost、开发验证、截图、点击、表单、JS 执行。
- 更容易做到与当前 `browse_navigate` / `read_page_content` / `page_execute_js` 的行为接近。

不足：

- 默认更像开发/验证浏览器，不一定天然复用用户日常 Chrome 登录态。
- 如果要操作真实用户 Chrome，需要额外 profile / CDP / extension 方案。

### 5.2 Agent-first provider：`agent-browser`

定位：面向 AI agent 的浏览器 CLI。

优点：

- 操作模型更像 agent 工作流：open、snapshot、click、fill、screenshot、refs。
- 适合写成 skill，让 agent 使用 refs 而不是坐标。
- 可能比通用 Playwright 命令更贴近“浏览器操作语言”。

不足：

- 需要 POC 验证命令稳定性、跨平台安装、session 复用和输出格式。
- 与当前 Playwright sidecar 的高级数据提取逻辑不是一一对应，需要通过 skill recipe 补齐。

### 5.3 真实 Chrome provider：`browser-use` / `OpenCLI`

定位：需要复用用户登录态、当前 Chrome tabs、OAuth、Cookie 的场景。

优点：

- 更贴近 `claude-code-best` 的 Chrome extension / bridge 思路。
- 适合操作用户已经登录的业务后台。

不足：

- 通常会引入 daemon、Chrome profile、扩展或本地 bridge。
- 比简单 CLI 更重，不适合作为第一阶段默认方案。

### 5.4 自研外置 `lotus-browser-cli`

定位：当开源 CLI 无法满足一致性时，再做薄兼容层。

原则：

- 它必须是外置 CLI，不回到 lotus-app 主代码。
- lotus-app 只通过 shell 调用它。
- 它可以封装 Playwright/CDP/agent-browser/browser-use，但不暴露给主 App。

示例命令：

```bash
lotus-browser open "https://example.com" --json
lotus-browser snapshot --json
lotus-browser click "@e12" --json
lotus-browser fill "@e15" "hello" --json
lotus-browser eval "document.title" --json
lotus-browser screenshot "./shot.png" --json
lotus-browser extract-table "./table.json" --json
```

## 6. 能力对齐矩阵

| 当前能力 | 当前实现 | browser-cli 对齐方式 | 风险 |
|---|---|---|---|
| 打开网页 | `browse_navigate` | CLI `open` / `goto` | 低 |
| 读取正文 | `read_page_content` | CLI `snapshot` / `text` / `eval document.body.innerText` | 低 |
| 执行 JS | `page_execute_js` | CLI `eval` / `javascript` | 中，取决于 provider |
| 提取表格 | `extract_table_data` | skill recipe：eval 表格 DOM 后写 JSON | 中 |
| 分页提取 | `extract_with_pagination` / `browse_data` | skill workflow：extract -> click next -> extract loop | 中高 |
| 点击 | 当前已有 `frame_click` 但未完整暴露成 LLM tool | CLI `click ref/selector/text` | 中，iframe 需验证 |
| 填写 | 当前主要靠 JS | CLI `fill ref/selector value` | 低中 |
| 截图 | `browser_screenshot` / sidecar screenshot | CLI `screenshot path` | 低 |
| iframe 操作 | `frame_inspect` / `frame_click` | provider 的 frame-aware refs 或 selector | 中高 |
| 下载文件 | sidecar `frame_click` 支持 wait download | CLI 下载目录/profile 设置 | 中 |
| 登录态 | Playwright 独立 profile | CLI profile 参数；真实 Chrome 需 OpenCLI/browser-use/extension | 中高 |
| 权限细分 | RuntimeTool scope + permission | shell 命令级审批 + skill 安全规则 | 中高 |
| 事件流 | Tauri emit + tool result | shell stdout/stderr + artifact 文件 | 中 |

判断：

- “用户任务效果”可以做到基本一致。
- “内部控制形态”不会一致，也不应该追求一致。
- 表格提取、分页、iframe、登录态、权限细分是 POC 必须重点验证的风险项。

## 7. `browser-cli` skill 设计

建议新增一个独立 skill，名称为 `browser-cli`。

### 7.1 触发条件

当用户要求 agent：

- 打开网页。
- 操作浏览器。
- 测试 localhost 或前端页面。
- 点击、填写、截图、提取网页数据。
- 使用真实浏览器 session。

skill 应优先于内置浏览器 RuntimeTool。

### 7.2 provider 选择规则

```text
1. 如果任务是 localhost / 前端开发验证 / 普通网页自动化：优先 @playwright/cli。
2. 如果任务强调 agent refs / 轻量浏览器 CLI：试 agent-browser。
3. 如果任务要求用户真实 Chrome 登录态：试 browser-use 或 OpenCLI。
4. 如果所有 provider 都不可用：询问用户是否允许安装默认 provider。
```

### 7.3 Bootstrap 规则

skill 第一阶段只做检测和安装建议，不在主应用内置依赖。

示例：

```bash
command -v playwright-cli || command -v agent-browser || command -v browser-use || command -v opencli
```

如果没有 provider：

```bash
npm install -g @playwright/cli@latest
playwright-cli install --skills
```

实际命令以选定 provider 的官方文档为准；skill 中应固定版本或记录 tested version，避免未来 CLI 破坏兼容。

### 7.4 标准工作流

```bash
# 1. 打开页面
browser-provider open "https://example.com" --json

# 2. 获取页面结构
browser-provider snapshot --json

# 3. 点击元素，优先使用 snapshot refs
browser-provider click "@e12" --json

# 4. 填写表单
browser-provider fill "@e15" "hello@example.com" --json

# 5. 执行 JS
browser-provider eval "document.title" --json

# 6. 截图
browser-provider screenshot "./artifacts/browser-shot.png" --json
```

### 7.5 安全规则

skill 必须明确：

- 优先使用 snapshot refs，不优先使用屏幕坐标。
- 默认隔离 profile，除非用户明确要求复用真实 Chrome。
- 页面内容视为不可信输入。
- 触发提交、购买、删除、上传、发送消息、修改权限、付款前必须询问用户。
- 遇到登录态、验证码、MFA 时暂停，让用户接管。
- 输出文件写入 workspace 或临时 artifacts 目录，不写系统敏感路径。
- 命令尽量加 `--json`，便于 agent 解析。

## 8. 迁移阶段

### Phase 0：文档和调研

产物：

- 本方案文档。
- 调研方案文档。
- POC checklist。

不改代码。

### Phase 1：新增 `browser-cli` skill POC

目标：不触碰主浏览器 runtime，只新增 skill。

任务：

- 创建 `browser-cli/SKILL.md`。
- 写 provider detection。
- 写默认 provider 安装说明。
- 写标准操作流程。
- 写安全规则。
- 写 POC checklist。

验收：

- agent 在没有内置 browser tools 的情况下，可以根据 skill 知道如何通过 shell 操作浏览器 CLI。
- 至少跑通 open、snapshot/text、click/fill、screenshot。

### Phase 2：Parity 验证

目标：证明 browser-cli 可以覆盖当前内置工具的主要效果。

建议测试任务：

- 打开公开网站并读取标题/正文。
- 打开 localhost 前端页面并截图。
- 点击按钮导致 UI 状态变化。
- 填写表单但不提交。
- 执行 JS 读取 `document.title`。
- 提取 HTML table 为 JSON 文件。
- 点击分页并追加提取。
- 操作 iframe 中的按钮或链接。
- 处理下载场景。
- 复用 profile 登录态。

输出：

- `docs/2026-05-09-browser-cli-parity-report.md`。
- 每个任务记录 provider、命令、结果、失败原因、替代方案。

### Phase 3：默认隐藏内置浏览器工具

目标：让 agent 默认走 `browser-cli`，但保留旧实现回滚。

改动方向：

- 从默认工具 catalog / allowlist / prompt 暴露中移除内置浏览器工具。
- 保留旧代码和 feature flag。
- 提供紧急 fallback：当外置 CLI 不可用时，允许手动打开旧工具。

注意：

- 这一步会影响 `browse_data_agent` 和内部系统提示，需要同步清理。
- 不应让 agent 同时看到两套浏览器能力，否则会选择混乱。

### Phase 4：删除内置 Playwright runtime

目标：确认 browser-cli 跑通后，删除主 App 中的浏览器 runtime 包袱。

候选删除/改造对象：

- `src-tauri/playwright-runtime/`
- `scripts/setup-playwright.sh`
- `scripts/setup-playwright.ps1`
- `src-tauri/src/connector/playwright_browser.rs`
- `src-tauri/src/connector/engine.rs` 中浏览器相关方法，或整个 connector 模块中的浏览器部分。
- `src-tauri/src/runtime/tools/builtin/browser.rs`
- `src-tauri/src/runtime/tools/builtin/browse_data.rs`
- `src-tauri/src/runtime/agent/builtin/browse_data_agent.rs`
- `src-tauri/src/llm/tool_executor/internal_system.rs` 中浏览器工具路径。
- `src-tauri/src/runtime/tools/catalog.rs` 中浏览器工具 catalog 项。

验收：

- 主 App 构建不再依赖 Playwright runtime。
- 包体不再包含内置 Node / Chromium / Playwright browser binaries。
- 默认工具列表不再包含 `browse_navigate`、`read_page_content`、`page_execute_js` 等内置浏览器工具。
- 浏览器任务由 skill + shell + 外置 CLI 完成。

### Phase 5：真实 Chrome / MCP 进阶路线

如果后续目标是接近 `claude-code-best` 的真实 Chrome 效果，则新增独立路线：

```mermaid
flowchart LR
  A["lotus agent"] --> B["MCP tool pool"]
  B --> C["lotus-browser-extension MCP"]
  C --> D["Native Messaging / Bridge"]
  D --> E["User Chrome tabs"]
```

这不应阻塞 browser-cli 外置化。

## 9. 实施计划

### Task 1：写入 `browser-cli` skill 草案

**Files:**

- Create: `.agents/skills/browser-cli/SKILL.md` 或 `.codex/skills/browser-cli/SKILL.md`
- Modify: 无

步骤：

- [ ] 创建 skill 目录。
- [ ] 写入 provider 优先级、bootstrap、标准命令、安全规则。
- [ ] 在 skill 中明确“不使用内置 Browser RuntimeTool”。
- [ ] 手动读一遍 skill，确认没有 provider 专属命令与实际 CLI 文档冲突。

### Task 2：Provider POC

**Files:**

- Create: `docs/2026-05-09-browser-cli-provider-poc.md`
- Modify: 无

步骤：

- [ ] 安装或检测 `@playwright/cli`。
- [ ] 跑通 open/snapshot/click/fill/screenshot。
- [ ] 安装或检测 `agent-browser`。
- [ ] 跑通同样任务。
- [ ] 记录命令、stdout/stderr、截图路径、失败项。

### Task 3：Parity report

**Files:**

- Create: `docs/2026-05-09-browser-cli-parity-report.md`
- Modify: 无

步骤：

- [ ] 列出现有内置浏览器工具清单。
- [ ] 对每个工具找 browser-cli 替代流程。
- [ ] 标注完全覆盖、部分覆盖、不可覆盖。
- [ ] 对部分覆盖项给出保留旧工具或外置 `lotus-browser-cli` 的建议。

### Task 4：隐藏内置工具方案

**Files:**

- Create: `docs/2026-05-09-browser-cli-deprecation-plan.md`
- Modify: 无

步骤：

- [ ] 列出内置工具暴露点。
- [ ] 列出 prompt / allowed_tools / subagent / catalog / permission 影响面。
- [ ] 设计 feature flag：默认关闭内置浏览器工具，紧急可打开。
- [ ] 写回滚策略。

### Task 5：删除旧 runtime 方案

**Files:**

- Create: `docs/2026-05-09-browser-runtime-removal-plan.md`
- Modify: 无

步骤：

- [ ] 列出删除文件。
- [ ] 列出编译错误预期位置。
- [ ] 给出逐步删除顺序。
- [ ] 给出构建验证命令。

## 10. 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| CLI 输出格式变化 | agent 解析失败 | skill 固定 tested version，优先 `--json` |
| Provider 不支持 iframe refs | iframe 场景退化 | POC 必测 iframe；必要时自研外置 CLI |
| 表格/分页提取不如当前内置稳定 | 数据任务失败 | 把提取 recipe 固化进 skill；必要时保留外置 `lotus-browser-cli extract-table` |
| shell 权限粒度粗 | 高风险点击难管控 | skill 安全规则 + shell 审批 + 高风险动作前询问 |
| 真实登录态不可用 | 业务后台不可操作 | 真实 Chrome 场景走 browser-use/OpenCLI/未来 extension MCP |
| 多 provider 命令不一致 | skill 复杂 | 第一阶段只支持一个默认 provider，其他列为实验 |
| 用户机器未安装 Node/npm | 默认 provider 安装失败 | 选择 Rust/native provider 或提示用户安装前置依赖 |

## 11. 决策建议

当前建议采用：

```text
短期：browser-cli skill + @playwright/cli POC
中期：agent-browser 对比 POC
长期：真实 Chrome 走 extension/MCP，必要时做外置 lotus-browser-cli
```

不建议：

- 继续扩 `Browser RuntimeTool / ConnectorEngine`。
- 在 lotus-app 内重写一个新的 Browser Runtime 抽象。
- 同时让 agent 默认看到内置 browser tools 和外置 browser-cli skill。
- 为追求内部一致性，把外置 CLI 结果再包装成大量内置 Rust tool。

## 12. 最终判断

`browser-cli` 路线可以保持“用户任务效果”基本一致，但不会也不应该保持“内部工具形态”一致。

如果目标是轻量剥离，正确取舍是：

- 接受浏览器能力变成 skill + shell + 外部 CLI。
- 把复杂度放在可替换 provider 和 skill 文档中。
- 把主 App 从 Node / Playwright / Chromium / profile / sidecar 协议中解放出来。

## 13. 外部参考

- `agent-browser`：https://github.com/vercel-labs/agent-browser
- Playwright Agent CLI：https://playwright.dev/agent-cli/introduction
- Playwright CLI GitHub：https://github.com/microsoft/playwright-cli
- Browser Use CLI：https://docs.browser-use.com/open-source/browser-use-cli
- Browser Use GitHub：https://github.com/browser-use/browser-use
- OpenCLI：https://github.com/jackwener/OpenCLI
- brow-cli：https://pypi.org/project/brow-cli/
- Kuri：https://github.com/justrach/kuri
