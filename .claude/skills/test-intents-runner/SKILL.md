---
name: test-intents-runner
description: Use when running, executing, or troubleshooting AIjia intent tests. Triggers from inside usertest-intents skill routing: "跑一下 X 这个 task", "跑 意图-XXX-NNN", "跑全部意图测试", "意图测试 FAIL 怎么处理", "tauri-pilot aijia ..." commands.
license: Internal
---

# test-intents-runner

## What this skill does

**怎么读 + 跑一条意图、怎么处理失败、怎么写报告**的完整方法论 + `tauri-pilot aijia` CLI 工具箱 + 已知 quirks 经验库。本 skill 不负责写 / 改意图（那是 `test-intents-author`）。

## When to Use

- 跑某个 task（如「跑一下日程」）
- 跑单条意图（如「跑 意图-日程-001」）
- 跑全部 task 回归
- 处理意图测试 FAIL 的诊断 / triage
- 解释 `tauri-pilot aijia ...` 命令

## 0. 执行流程总览（被调起后从头看到尾）

接到「跑 X 意图」/「跑 X task」后，按下面 6 阶段顺序执行：

```
┌─ 阶段 1: 环境预检（§4.1）─────────────────────┐
│  - 检查 vite 是否在 5173 跑（lsof -ti tcp:5173）│
│  - 检查 tauri-pilot socket 是否就绪          │
│  - 没跑 → 提示用户起 `pnpm dev:with-pilot`   │
│    并等到首次 build 通过（首次 cargo 编译可能│
│    几分钟），不能用 `pnpm tauri:dev`         │
│  - socket 残留 → `rm -f /tmp/tauri-pilot-*`  │
└──────────────────────────────────────────────┘
                    ↓
┌─ 阶段 2: 探活 + 推断 scope ──────────────────┐
│  - tauri-pilot aijia health-check --json     │
│  - tauri-pilot aijia where --json 取 scope   │
│  - 失败 → 报「FAIL 主因 = rules/CLI 问题」  │
│    停止后续，不进阶段 3                       │
└──────────────────────────────────────────────┘
                    ↓
┌─ 阶段 3: 读 rules.md（§1）───────────────────┐
│  - 读 docs/test-intents/spec/tasks/<task>/   │
│    rules.md，定位要跑的意图                   │
│  - 跑全 task = 按 ID 升序逐条跑               │
│  - 跑单条 = 找到对应 ID 块                    │
└──────────────────────────────────────────────┘
                    ↓
┌─ 阶段 4: 逐条跑（§2）───────────────────────┐
│  - 顺序执行「操作步骤」段                     │
│  - 中途某步挂了 → 现场判断后续是否继续         │
│    跑（§2.1）；验收永远跑                     │
│  - 全 task：失败不串联，跑完所有意图才出报告 │
└──────────────────────────────────────────────┘
                    ↓
┌─ 阶段 5: 核验 → 写报告（§3）─────────────────┐
│  - 对每条意图核验 ✅ / ❌ 段                  │
│  - 标 PASS/FAIL/SKIPPED + FAIL 主因三选一    │
│  - 报告**直接在对话里输出**，不落盘          │
└──────────────────────────────────────────────┘
                    ↓
┌─ 阶段 6: 经验沉淀建议（§5）──────────────────┐
│  - 跑出新陷阱 / 新诊断套路 → 在报告末尾建议  │
│    「这条经验值得沉淀到 runner skill §5」     │
│  - 由人决定 PR 加进来（你不直接改 skill）    │
└──────────────────────────────────────────────┘
```

**铁则**：不要跳阶段。阶段 1 没过就直接跑 CLI = 必失败。

## 1. 怎么读一条意图

`rules.md` 每条意图固定 3 段，agent 读时这样用：

| 段 | agent 怎么用 |
|---|---|
| **场景** | 1-3 句产品视角描述。读懂这条意图测的是什么承诺，和你的执行预期对齐 |
| **操作步骤** | 编号步骤，从第 1 步顺序跑到最后一步；**第一步永远是** `tauri-pilot aijia health-check` 探活；后续含搭环境命令 + 主测操作 |
| **验收标准** | `✅ 应该看到` 列 PASS 条件、`❌ 不应该看到` 列必须主动检查的反向陷阱 |

注意：
- **没有「前提」段**——搭环境步骤就在「操作步骤」最前面
- **没有「判定提示」段**——边界容忍写进验收的容忍范围（`T0 ± 1 分钟`）；通用诊断套路在本 skill §5 经验库

## 2. 跑的执行语义

### 2.1 单条意图

按「操作步骤 → 验收标准」顺序执行。

**中途某步挂了的处理**——用产品视角现场判断「后续是否还有意义」：

| 情况 | 做法 |
|---|---|
| 后续步骤的前提因这一步失败而不成立（如点不开侧栏 → 后续点不到东西） | **跳过后续步骤** |
| 后续步骤可独立（如某个等待 timeout 但实际状态可能已 OK） | **继续跑后续步骤** |
| 任何步骤挂 | **验收永远要跑**——挂在哪一步都核验 ✅ / ❌ 段，看现场磁盘 / UI 状态对 triage 有用 |

### 2.2 报告 FAIL 主因必须三选一

| 主因 | 含义 | 修哪里 |
|---|---|---|
| `rules/CLI 问题` | 步骤本身写错 / CLI 接口变了 / selector 找不到 | 改 rules.md 或 `aijia` CLI |
| `产品 bug` | 被测应用真的卡住 / 状态错乱 | 改产品代码 |
| `待 triage` | 不确定时写这个 | 请人判 |

### 2.3 跑全 task 的执行语义

- **串行**——一次只跑一条意图，**不并发**
- **失败不串联**——某条意图 FAIL 不影响后续意图跑；必须跑完所有意图才出报告
- **每条意图独立可跑**已由 spec §3.6 保证（意图自给自足、无跨意图依赖），前一条 FAIL 不会污染后一条前置状态
- 报告里**按 ID 升序逐条**列结果（PASS/FAIL/SKIPPED），**不汇总成 task 级别的「整体 PASS/FAIL」**——意图测试不做总分

### 2.4 不分失败等级

FAIL 就是 FAIL，**不分 P0 / P1 / P2**；轻重由人看报告内容判。

### 2.5 不规定 FAIL → issue 流程

FAIL 之后开不开 issue / 改实现 / 改 rules，由人按 FAIL 主因 + 自身判断处置。意图测试只产报告，**不联动外部 issue 系统**。

## 3. 报告输出格式

agent 跑完意图测试，**直接在对话里向调起者输出结构化报告**，不落盘到仓库：

```markdown
# 日程意图测试报告 — 2026-05-20 14:30

**应用版本**: v0.5.26
**登录账号**: pzc@renlijia.com（scope `t_xxx__u_yyy`）
**本轮跑了**: 意图-日程-001, 意图-日程-002, 意图-日程-003

## 意图-日程-001: 创建一次性日程后落盘 — ✅ PASS（45s）
- 操作按 rules.md 14 步执行无异常
- ✅ 应该看到：表单收起 ✓ / 列表条目 ✓ / JSON 字段全对 ✓
- ❌ 不应该看到：无旧字段名 ✓ / 列表无重复 ✓
- 备注：无

## 意图-日程-002: 一次性日程到期自动触发 — ❌ FAIL（120s）
- 操作按 rules.md 执行到第 8 步「等到 T0+4 分钟」时 wait timeout
- 实际现象：到点 +60s 仍未触发，`agenda/occurrences/` 目录无新文件
- **FAIL 主因** = 产品 bug
- 建议：开 issue 调查 agenda runner 调度逻辑

## 意图-日程-003: 用户点立即运行 — ⏭️ SKIPPED
- 因 意图-日程-002 涉及调度系统 FAIL，本条独立性已由 §3.6 保证、本可跑；
  作者在对话里说「先停下别跑了」，故 SKIPPED
```

**不持久化**：报告在对话里给调起者看完即关闭。下次跑从零开始，**不读历史**——读历史会带预设。

**沉淀经验建议**：如果跑出新陷阱 / 新诊断套路 / 新容忍判定，**在报告末尾建议「这条经验值得沉淀到 test-intents-runner skill §5」**，由人决定 PR 加进来。

## 4. CLI 工具箱：`tauri-pilot aijia` 16 子命令

### 4.1 启动连通性

⚠️ **必须用 `pnpm dev:with-pilot` 而不是 `pnpm tauri:dev`**——`tauri-plugin-pilot` 在 `src-tauri/Cargo.toml` 里是 `optional = true`，只在 `e2e` cargo feature 开启时才会被 `src-tauri/src/lib.rs` 注册（`#[cfg(feature = "e2e")]`）。`pnpm tauri:dev` 默认不带 e2e feature，**plugin-pilot 不会被注册、socket 永远不存在**，CLI 全部命令会报 "No tauri-pilot socket found"。

`pnpm dev:with-pilot` 等同于 `tauri dev --features e2e`：

```bash
cd ~/IdeaProjects/lotus-app
rm -f /tmp/tauri-pilot-com.aijia.app.sock        # 清理可能的残留 socket

# 启动带 e2e feature 的 dev server（首次 cargo 编译可能 5-10 分钟）
pnpm dev:with-pilot &

# 等 vite 起来
until lsof -ti tcp:5173 >/dev/null; do sleep 2; done

# 等 tauri build + socket 就绪（cargo 编译完才会有 socket）
until [ -S /tmp/tauri-pilot-com.aijia.app.sock ]; do sleep 2; done

# 探活
tauri-pilot aijia health-check --json   # → {"ok":true,"readyState":"complete",...}
```

如果 dev server 在跑但 socket 不存在，多半是用了错误的命令 `pnpm tauri:dev`——停掉 + 重起 `pnpm dev:with-pilot`。不要尝试同时跑两个 dev server 抢 cargo lock。

### 4.1b 也可以用 release 包跑意图测试（自 2026-05-25 起）

如果你有 `pnpm build:with-pilot` 出来的 release 包（`src-tauri/target/release/bundle/macos/AIjia.app`），可以直接 `open` 它代替起 dev server。Release 包跟 dev 模式**接口完全等价**——同样的 socket 路径、同样的 `aijia` CLI 子命令。差异只是：

- ✅ 不需要 vite / cargo 跑着（启动更快、不占编译器）
- ✅ 体感跟 QA 真实场景一致（带 bundled runtime、release 优化）
- ⚠️ 修改 lotus-app 代码不会热更新，要重 `pnpm build:with-pilot`（每次 5-7 分钟）

实操：
```bash
rm -f /tmp/tauri-pilot-com.aijia.app.sock
open /Users/a20250311/IdeaProjects/lotus-app/src-tauri/target/release/bundle/macos/AIjia.app
until [ -S /tmp/tauri-pilot-com.aijia.app.sock ]; do sleep 1; done
tauri-pilot aijia health-check --json
```

发版流程 + release build 工程细节见 `docs/e2e-release-build.md`。

### 4.2 命令清单

所有命令默认 stdout 一行 JSON（加 `--json`）。

#### 会话流（P0）

| 命令 | 作用 |
|---|---|
| `aijia new-task` | 新建空对话 |
| `aijia type-message <text>` | Tiptap `execCommand('insertText', ...)` 输入 |
| `aijia send` | 点发送 |
| `aijia wait-reply [--timeout 30]` | 阻塞等流式结束（**stability window: 3 连续 ready tick**） |
| `aijia ui-message [--last N] [--role user\|assistant\|tool_call] [--since 2m] [--include-tools]` | dump DOM 消息 |
| `aijia last-reply` | `ui-message --last 1 --role assistant` 别名 |

#### 会话管理（P1）

| 命令 | 作用 |
|---|---|
| `aijia list-sessions` | 列侧栏所有会话 `[{id, title, index, active, archived}]` |
| `aijia switch-session <id\|index>` | 切对话（数字 0=最新） |
| `aijia archive-session <id\|index>` | IPC `archive_conversation`（**不走 UI hover**） |
| `aijia cleanup-test-sessions [--prefix e2e-test-]` | 批量归档 title 前缀匹配的会话（**只匹配 title**） |

#### 流式取消（P2）

| 命令 | 作用 |
|---|---|
| `aijia cancel` | 流式中点停止；流式未开始时报错 |

#### 诊断

| 命令 | 作用 |
|---|---|
| `aijia where` | dump 现场 `{url, title, route, activeConversationId, messageCount, isStreaming, hasEditor}`——**失败时第一步跑这个** |
| `aijia screenshot [--label <label>]` | 截图到 `/tmp/aijia-e2e-{label}-{ts}.png` |
| `aijia health-check` | app ready 探测（启动后第一个跑） |

#### 未实现（不要在 rules.md 里依赖）

`aijia select-workspace` / `aijia restart-app` — 当前返回 `not implemented`。

### 4.3 铁则

**e2e 脚本只走 `aijia` 子命令，禁直接 `click` / `eval`**（来源：`MEMORY.md project_e2e_testing_tauri_pilot.md`）。

一条 rule 无法用现有 16 个 CLI 表达时，**应该新加 `aijia` 子命令**而不是绕过用 raw CLI。这种情况下：
1. 报告里写明「rule X 第 N 步无对应 CLI」
2. 建议产品方加新子命令（写进报告"经验沉淀"建议）
3. 暂时把这条意图标为 `FAIL 主因 = rules/CLI 问题`，不强跑

### 4.4 与 rules.md 对接（最小回合）

```bash
tauri-pilot aijia new-task
tauri-pilot aijia type-message "你好"
tauri-pilot aijia send
tauri-pilot aijia wait-reply --timeout 60
reply=$(tauri-pilot aijia last-reply --json | jq -r .text)
conv=$(tauri-pilot aijia where --json | jq -r .activeConversationId)

[ -n "$reply" ]
test -d ~/.renlijia/users/$scope/conversations/$conv/
```

## 5. 已知 quirks 经验库（持续追加）

`rules.md` 是单条意图精确规约；本节是跑了很多意图后沉淀的**横切知识**。属于本 skill 不属于 task。

agent 跑意图时遇到新陷阱 / 新诊断套路 / 新容忍判定，**在报告末尾建议「这条经验值得沉淀到本 skill §5」**，由人决定 PR 加进来。

### 5.1 `pnpm tauri:dev` 不带 e2e feature，**必须** `pnpm dev:with-pilot`

`tauri-plugin-pilot` 在 `src-tauri/Cargo.toml` 里是 `optional = true`，只有开启 `e2e` cargo feature 才会被 `src-tauri/src/lib.rs::run()` 注册（`#[cfg(feature = "e2e")]`）。

- ❌ `pnpm tauri:dev` —— 不带 feature，plugin-pilot 不注册，socket 永不存在
- ✅ `pnpm dev:with-pilot` —— `package.json` 里专为 e2e 准备的脚本，等同于 `tauri dev --features e2e`

**症状识别**：所有 `tauri-pilot aijia ...` 命令一律报 `Error: No tauri-pilot socket found. Is a Tauri app running?`——多半是用错命令。

**首次跑会很慢**：开启 e2e feature 会从 GitHub 拉 `tauri-plugin-pilot`（https://github.com/panzhenchao/tauri-pilot.git），初次 cargo 编译 5-10 分钟，不要中途打断。

### 5.2 wait-reply 必须用 stability window

`aijia wait-reply` 用 3 连续 `isStreaming === false` 才算流结束——单点采样会在 tool_calls 之间误判完成。工具调用密集的 turn，默认 30s timeout 可能不够，按需调 `--timeout 60` 或更长。

### 5.3 archive 后 list-sessions 不会自动刷新

`aijia archive-session` 走 IPC：磁盘 `conv.json.isArchived=true` 已更新，但侧栏 DOM 还显示老状态。要么 sleep + 触发别处刷新，要么直接读 `~/.renlijia/.../conv.json` 验证。

### 5.4 cleanup-test-sessions 只看 title 前缀

`aijia cleanup-test-sessions --prefix e2e-test-` 只匹配 title，发消息**不会更新 title**——想匹配必须先 rename。

### 5.5 screenshot 30s 超时绕道

`aijia screenshot` 可能 30s 超时（wrapper 走 html-to-image）。用 raw `tauri-pilot screenshot <path>` 兜底（webview screenshot，~100ms 完成）。

### 5.6 socket 残留

应用 crash 后 `/tmp/tauri-pilot-com.aijia.app.sock` 不会被清理，**下次启动前 `rm -f` 一下**。

### 5.7 messages.jsonl 格式

单文件 ndjson，每行末尾 `\t✓` 校验位——解析时要先 `split('\t')[0]`。`content` 是 `{text: "..."}` 嵌套对象，不是字符串。**额外坑**：`content.text` 内的真实换行 `\n` 没被 JSON-escape 成 `\\n`，所以 `wc -l` / 按字面行号取 user / assistant 都会错位。真正的记录分隔符是 `\t✓\n`——按它 split 后的每段才是一条记录。`tool_calls` 在顶层（不在 `content.tool_calls`），tool_result 关联字段是 `toolCallId`（不是 `tool_use_id`）。

### 5.7b agenda occurrences jsonl 是 append-update 语义

每条 occurrence 在生命周期中会**写多行同 id 记录**：先在 `running` 状态写入一行、`succeeded`/`failed` 完成时再 append 新一行。验收时**必须 `tail -1`** 取最新一条而非首条；按行数判定 occurrence 数量会虚高（一条 manual_run_now 看上去像 2 条 occurrence）。判 `occurrenceCount` 时应该读 items 文件里的字段，不是数 jsonl 行。

### 5.8 list-sessions 字段命名

实际返回 `{id, title, index, active, archived}`（不是 `name` / `isActive` / `isArchived`）。

### 5.9 last-reply 返回字段是 `text` 不是 `content`

实际是 `{id, index, role, text, tool_calls}`。

### 5.10 跨工作目录测试不支持

`aijia select-workspace` 未实现，沿用当前 workspace。

### 5.11 改 bridge.js 后 cargo 不会自动重编

修改 `crates/tauri-plugin-pilot/src/bridge.js` 后，`touch crates/tauri-plugin-pilot/src/lib.rs` 强制重新 `include_str!`。

### 5.12 `wait-reply` 超时多半是授权弹窗卡住，不是 LLM 慢

`wait-reply --timeout 90` 超时后**先怀疑授权弹窗**：某些 tool（ReadFile 跨目录、上传文件、外部 HTTP 等）首次执行会弹权限对话框等用户同意，UI 期间 `isStreaming: true` 但流不前进。诊断顺序：
1. `aijia where --json` 看 `isStreaming` 还是 true → 截图 `aijia screenshot --label timeout-debug` 看屏幕
2. 看到弹窗 → **手动点同意/记住选择**，再 `wait-reply --timeout 60` 续等
3. 没弹窗、`isStreaming` 还在 true → 看 `messages.jsonl` 时间戳：相邻两条 assistant/tool 行间隔 > 60s 就是真有东西卡住，不是 stability window 没捕到

实战来源：项目记忆 task 意图 2，SearchMemory 完成后到 ReadFile 触发之间空档 **3 分 37 秒**（07:43:15 → 07:46:52），跨 workspace 路径访问的授权弹窗在等同意。`wait-reply` 阻塞期间无法看到弹窗，必须先 `aijia where --json` + screenshot。

**长期方向**：`aijia` 应加 `auto-approve-permissions` / 启动期 e2e 模式禁用所有需要交互的 permission ask，避免每次跑测都被弹窗截胡。

### 5.13 `aijia new-task` 切到 home 路由，conv id 懒创建

`aijia new-task` 调用后立刻 `aijia where --json` 拿到 `route: "home"`、`sessionId: null`（或 stale 的上一个 active sessionId）、stale 的 `messageCount`。新对话 ID 要等 `aijia send` 真把第一条消息发出去后才生成。rules.md 写"新建对话"步骤的 CLI 序列必须是 `new-task` → `type-message` → `send`，**send 之后**再 `where --json` 才能取到新 `sessionId` 作为 `$CONV_ID`。

实战来源：意图-对话-001 跑测（2026-05-29），new-task 后 where 返回 home + stale `messageCount=162`（上一个 active conv 的字段），send 后才出现新 conv id。

## 6. 环境契约

- 直接在真实 `~/.renlijia/` 跑、**不**隔离、跑后**不**清理
- scope 从 `tauri-pilot aijia where --json` 推断
- 要纯净测试环境，**开新电脑 / 新用户重新登**，不靠代码做 sandbox
- 意图测试产生的真实对话 / 真实 LLM 调用 / 真实日程 / 真实文件**会落到你账号下**——有意的设计

## Red Flags（看到自己在这样想就停下来）

| 想法 | 应该 |
|---|---|
| 「这条意图应该可以用 `eval` / `click` 偷懒」 | 不行——只走 `aijia` 子命令；要么新加子命令 |
| 「上次 FAIL 是因为 X，我直接判 FAIL 吧」 | 不行——每次按 rules.md 客观跑、看现场，不读历史 |
| 「这步挂了后面没意义跑，验收也不用了吧」 | 不行——**验收永远跑**，看现场 |
| 「跑全 task 时第 3 条 FAIL 后续就别跑了」 | 不行——FAIL 不串联，必须跑完所有意图 |
| 「这条 FAIL 是真 bug 还是测试问题不确定，先写 PASS 吧」 | 不行——写 `FAIL 主因 = 待 triage`，请人判 |
| 「报告写得长点显得我跑得认真」 | 不必——按 §3 格式精确、关键 FAIL 主因写到即可 |
| 「我顺手在仓库写一份 progress.md 留个记录」 | 不行——progress 已废除，报告只在对话里 |
