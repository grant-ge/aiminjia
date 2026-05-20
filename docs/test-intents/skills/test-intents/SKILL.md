---
name: test-intents
description: Use when running, writing, or modifying intent tests for the AIjia desktop app. Triggers include "跑意图测试", "跑一下 X 这个 task", "跑 意图-XXX-NNN", "写意图", "加一条意图", "改 rules.md", "意图测试", "AEIT", or any usage of the tauri-pilot aijia CLI for end-to-end testing.
license: Internal
---

# test-intents

## Overview

意图测试 = **AEIT**（Agent-Executed Intent Test）= 在真实开发机的真实账号下，agent 跑一遍真实操作，留下真实痕迹。仅在 L4（tauri-pilot e2e + 真应用 + 真 LLM + 真磁盘）跑，**不**在 cargo / vitest 跑。

权威设计：`docs/superpowers/specs/2026-05-20-intent-test-redefinition-design.md`（以下简称**设计 spec**）。本 skill 是该 spec 的可执行投影——agent 跑 / 写意图时按本 skill 即可。

## When to Use

agent 被人调起做这些事时使用：
- 跑某个 task（如「跑一下日程」）→ §2 怎么跑
- 跑单条意图（如「跑 意图-日程-001」）→ §2 怎么跑
- 加 / 改 / 删意图 → §3 怎么写
- 解释 `tauri-pilot aijia ...` 命令 → §4 CLI 工具箱
- 失败诊断 / 边界判定 → §7 执行经验库

**Not for**：cargo 单测、vitest 组件测试、L2 review tests——这些不属于意图测试范畴。

## 1. 核心约束（写 / 跑前都要知道）

- **环境契约**（设计 spec §3）：直接在真实 `~/.renlijia/` 跑、不隔离、不开 test mode；跑完不清理（前提里自己清）；scope 从 `tauri-pilot aijia where --json` 推断。要纯净环境去新电脑。
- **不持久化**（设计 spec §4）：报告**只在对话里**输出给调起者，仓库内无 `progress.md`；下次跑从零开始。
- **意图自给自足**（设计 spec §3.6）：意图之间**无顺序依赖**；需要某状态就在自己的「操作步骤」段最前面搭出来，禁止「先跑 意图-XXX」这种引用。

## 2. 怎么读 + 跑一条意图

`rules.md` 每条意图固定 3 段：**场景**（PM 视角描述）/ **操作步骤**（编号步骤，第 1 步永远是 `tauri-pilot aijia health-check`，含搭环境 + 主测）/ **验收标准**（`✅ 应该看到` + `❌ 不应该看到`）。

按操作步骤顺序跑，跑完核验验收。

### 中途失败的处理

| 情况 | 做法 |
|---|---|
| 后续步骤的前提因这一步失败而不成立（如点不开侧栏） | 跳过后续步骤 |
| 后续步骤可独立 | 继续跑 |
| 任何步骤挂 | **验收永远要跑**——看现场磁盘 / UI 状态对 triage 有用 |

### 跑全 task

- **串行**（一次只跑一条），不并发
- **失败不串联**——某条 FAIL 不影响后续意图跑；必须跑完所有意图才出报告
- 按 ID 升序逐条列结果（PASS/FAIL/SKIPPED），**不汇总 task 级别整体 PASS/FAIL**

### 报告格式

```markdown
# 日程意图测试报告 — 2026-05-20 14:30

**应用版本**: v0.5.26
**登录账号**: pzc@renlijia.com（scope `t_xxx__u_yyy`）
**本轮跑了**: 意图-日程-001, 意图-日程-002

## 意图-日程-001: 创建一次性日程后落盘 — ✅ PASS（45s）
- 操作按 rules.md 14 步执行无异常
- ✅ 应该看到：表单收起 ✓ / 列表条目 ✓ / JSON 字段全对 ✓
- ❌ 不应该看到：无旧字段名 ✓ / 列表无重复 ✓

## 意图-日程-002: ... — ❌ FAIL（120s）
- 操作执行到第 8 步「等到 T0+4 分钟」时 wait timeout
- 实际现象：到点 +60s 仍未触发，`agenda/occurrences/` 无新文件
- **FAIL 主因** = 产品 bug
- 建议：开 issue 调查 agenda runner 调度逻辑
```

**FAIL 主因必须三选一**：`rules/CLI 问题` / `产品 bug` / `待 triage`（不确定时）。

**不分 P0/P1/P2 失败等级**，**不规定 FAIL → issue 流程**——开不开 issue 由人按报告判。

## 3. 怎么写 / 改一条意图

### 3.1 ID 与排序

格式 **`意图-<task>-<NNN>`**：`<task>` 中文名、`<NNN>` 三位、新意图 = 当前最大序号 + 1、**删除不回收 ID**、按 ID 升序排列、新意图 append 到文件末尾。

**作废 = 硬删除整段**，不留废弃标记 / 占位 / 替代指针。

### 3.2 标题命名（4 条硬规则）

格式 **`<触发条件>，<可观察结果>`**——中文逗号分隔两段。

1. 两段、超过 2 段说明复合、回去拆
2. 不带技术名（不写 React 组件名、IPC 命令、DOM selector）
3. ≤ 30 字（不含 `意图-<task>-NNN:` 前缀）
4. 不用 ✓ ✗ `/` 当连接符

### 3.3 「操作步骤」措辞规则

**禁用技术术语**：

| ❌ 不要 | ✅ 改成 |
|---|---|
| 等待 `AgendaItemEditor` sheet 打开 | 等新建表单展开（能看到「标题」「Prompt」「开始时间」等输入项） |
| 等待 `[data-testid="agenda-editor"]` 出现 | 同上 |
| 通过 IPC 调用 `employee_active_run` | 点击员工卡片右上角的「派活」按钮 |

等待信号写**用户能看到的 UI 文案**——agent 自己用 CLI 翻译成 poll 逻辑。

**一条意图一件事**：一条 `意图-<task>-<NNN>` 只能一组操作步骤 + 一组验收。

### 3.4 「验收标准」6 条书写规则

1. **每条 bullet 可机器观察**——禁主观词（"成功" "正常" "合理" "正确"）
2. **不复合**——一条 bullet 一件事
3. **6 种标准断言形式之一**：UI 出现 / UI 消失 / 文件存在 / 文件不存在 / 字段精确匹配 / 字段范围匹配
4. **路径用字面值或带 `{scope}` `{tenantId}` `{userId}` `T0` 4 个变量的明确模式**，禁形容词
5. **字段断言用 5 种运算符**：`==` / `!=` / `null|not null` / `length == N | length >= N` / 时间数值范围
6. **`❌ 不应该看到` 段断言本身是反向陈述**（"X 不存在"），允许写「无」但段头必留

### 3.5 「操作步骤」允许的命令（白/黑名单）

**白名单**：
- `rm -rf ~/.renlijia/users/{scope}/<具体子目录>/`
- `rm ~/.renlijia/users/{scope}/<具体glob>`
- `ls` / `test -d` / `test -f` / `stat`
- `mkdir -p ~/.renlijia/users/{scope}/<具体子目录>/`
- `echo "..." > ~/.renlijia/users/{scope}/<具体路径>`
- 所有 `tauri-pilot aijia` CLI 子命令

**黑名单**：
- 操作 `~/.renlijia/` 根目录 / 家目录 / 系统目录
- 操作 git 仓库（`git reset --hard` / `git clean -fd` 等）
- 启停应用 / 系统进程（`killall AIjia` / `pkill -9`）
- 任何 `sudo` 命令
- 网络操作（`curl` / `wget` / `ssh`）
- 修改环境变量 / shell profile

**例外**：`崩溃恢复/` task 允许 `kill <pid>` / `pkill AIjia`——但每条用到的意图在「操作步骤」段开头加 `⚠️` 警告 + agent 跑前**先和作者确认可以 kill 应用**。

### 3.6 写后自查清单（3 层 13 项硬伤）

写完每条意图按下列 3 层过一遍，命中即返工。**不要写「agent 自己判断一下」之类的兜底**。

**Layer 1 语义**：代指不清晰 / 表述模糊 / 标准过于宽泛 / 表述拗口
**Layer 2 结构**：非原子化 / 多约束硬塞 / 冗余 / 包含关系 / 标准冲突 / 格式不规范 / **意图标题违规**（超 30 字 / 含技术名 / 含符号连接）
**Layer 3 完备性**：遗漏关键约束 / 包含无关约束 / 标准本身错误（如 `status == "Active"` 而实际枚举是小写 `active`）

### 3.7 review 流程 = 对话即时 review，不走 PR

1. 在对话里和作者确认要加 / 改 / 删的意图
2. agent 按 §3.1-§3.6 写 rules.md diff
3. 在对话里把 diff 展示给作者
4. 作者**只 review 承诺方向**（这是产品承诺吗？该写在这个 task 吗？和现有意图冲突吗？）——形式合规 agent 自检
5. 通过 → commit；不通过 → 反馈重写

## 4. CLI 工具箱：`tauri-pilot aijia` 16 子命令

### 4.1 启动连通性

```bash
cd ~/IdeaProjects/lotus-app
rm -f /tmp/tauri-pilot-com.aijia.app.sock
pnpm tauri:dev &
until lsof -ti tcp:5173 >/dev/null; do sleep 2; done
tauri-pilot aijia health-check --json   # → {"ok":true,"readyState":"complete",...}
```

### 4.2 命令清单（所有命令默认 stdout 一行 JSON，加 `--json`）

**会话流**（P0）：
| 命令 | 作用 |
|---|---|
| `aijia new-task` | 新建空对话 |
| `aijia type-message <text>` | Tiptap `execCommand('insertText', ...)` 输入 |
| `aijia send` | 点发送 |
| `aijia wait-reply [--timeout 30]` | 阻塞等流式结束（**stability window: 3 连续 ready tick**） |
| `aijia ui-message [--last N] [--role user\|assistant\|tool_call] [--since 2m] [--include-tools]` | dump DOM 消息 |
| `aijia last-reply` | `ui-message --last 1 --role assistant` 别名 |

**会话管理**（P1）：
| 命令 | 作用 |
|---|---|
| `aijia list-sessions` | 列侧栏所有会话 `[{id, title, index, active, archived}]` |
| `aijia switch-session <id\|index>` | 切对话（数字 0=最新） |
| `aijia archive-session <id\|index>` | IPC `archive_conversation`（**不走 UI hover**） |
| `aijia cleanup-test-sessions [--prefix e2e-test-]` | 批量归档 title 前缀匹配的会话（**只匹配 title**） |

**流式取消**：`aijia cancel`（流式中点停止；流式未开始时报错）

**诊断**：
| 命令 | 作用 |
|---|---|
| `aijia where` | dump 现场 `{url, title, route, activeConversationId, messageCount, isStreaming, hasEditor}`——**失败时第一步跑这个** |
| `aijia screenshot [--name <label>]` | 截图到 `/tmp/aijia-e2e-{label}-{ts}.png` |
| `aijia health-check` | app ready 探测（启动后第一个跑） |

**未实现**（不要在 rules.md 里依赖）：`aijia select-workspace` / `aijia restart-app`

### 4.3 铁则

**e2e 脚本只走 `aijia` 子命令，禁直接 `click` / `eval`**（来源：`MEMORY.md project_e2e_testing_tauri_pilot.md`）。一条 rule 无法用现有 16 个 CLI 表达时，应该**新加 `aijia` 子命令**而不是绕过用 raw CLI。

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

## 5. 已知边界（执行经验库 — 持续追加）

`rules.md` 是单条意图精确规约；本节是跑了很多意图后沉淀的**横切知识**。属于本 skill 不属于 task。

agent 跑意图时遇到新陷阱 / 新诊断套路 / 新容忍判定，**在报告末尾建议「这条经验值得沉淀到本 skill §5」**，由人决定 PR 加进来。

### 5.1 wait-reply 必须用 stability window

`aijia wait-reply` 用 3 连续 `isStreaming === false` 才算流结束——单点采样会在 tool_calls 之间误判完成。工具调用密集的 turn，默认 30s timeout 可能不够，按需调 `--timeout 60` 或更长。

### 5.2 archive 后 list-sessions 不会自动刷新

`aijia archive-session` 走 IPC：磁盘 `conv.json.isArchived=true` 已更新，但侧栏 DOM 还显示老状态。要么 sleep + 触发别处刷新，要么直接读 `~/.renlijia/.../conv.json` 验证。

### 5.3 cleanup-test-sessions 只看 title 前缀

`aijia cleanup-test-sessions --prefix e2e-test-` 只匹配 title，发消息不会更新 title——想匹配必须先 rename。

### 5.4 screenshot 30s 超时绕道

`aijia screenshot` 可能 30s 超时（wrapper 走 html-to-image）。用 raw `tauri-pilot screenshot <path>` 兜底（webview screenshot，~100ms 完成）。

### 5.5 socket 残留

应用 crash 后 `/tmp/tauri-pilot-com.aijia.app.sock` 不会被清理，下次启动前 `rm -f` 一下。

### 5.6 messages.jsonl 格式

单文件 ndjson，每行末尾 `\t✓` 校验位——解析时要先 `split('\t')[0]`。`content` 是 `{text: "..."}` 嵌套对象，不是字符串。

### 5.7 list-sessions 字段命名

实际返回 `{id, title, index, active, archived}`（不是 `name` / `isActive` / `isArchived`）。

### 5.8 last-reply 返回字段是 `text` 不是 `content`

实际是 `{id, index, role, text, tool_calls}`。

### 5.9 跨工作目录测试不支持

`aijia select-workspace` 未实现，沿用当前 workspace。

### 5.10 改 bridge.js 后 cargo 不会自动重编

修改 `crates/tauri-plugin-pilot/src/bridge.js` 后，`touch crates/tauri-plugin-pilot/src/lib.rs` 强制重新 `include_str!`。

## Red Flags（看到自己在这样想就停下来）

| 想法 | 应该 |
|---|---|
| 「这条意图应该可以用 `eval`/`click` 偷懒」 | 不行——只走 `aijia` 子命令；要么新加子命令 |
| 「上次 FAIL 是因为 X，我直接判 FAIL 吧」 | 不行——每次按 rules.md 客观跑、看现场，不读历史 |
| 「这步挂了后面没意义跑，验收也不用了吧」 | 不行——**验收永远跑**，看现场 |
| 「这条意图标题太长了不优雅但意思到位」 | 不行——> 30 字必重写（§3.2） |
| 「写「字段值合理」就行，agent 能懂」 | 不行——禁主观词（§3.4 规则 1） |
| 「先跑 意图-XXX 我才能跑这条」 | 不行——意图自给自足（§1） |
| 「跑全 task 时第 3 条 FAIL 后续就别跑了」 | 不行——FAIL 不串联，必须跑完所有意图 |
| 「这条 FAIL 是真 bug 还是测试问题不确定，先写 PASS 吧」 | 不行——写 `FAIL 主因 = 待 triage`，请人判 |
