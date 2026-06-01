# `tauri-pilot aijia` CLI 缺失清单

> 来源：跑日程 + 项目记忆 两个 task 实战暴露的 gap
> 更新：2026-05-22
> 定位：补充 `tauri-pilot aijia` 业务子命令；底层仍走 webview DOM 操作（click / fill / eval），**不**改为直调内部 IPC

## 背景

按 runner skill 铁则「e2e 脚本只走 `aijia` 子命令，禁直接 click/eval」。目前 16 个子命令偏聊天流，跑除项目记忆之外的 task 几乎都被迫绕过铁则用 generic `tauri-pilot click/eval/fill`。本清单按优先级列出最缺的 11 个子命令。

每个子命令都是对 DOM 操作的封装（一组 click + fill + wait），**不是**调用内部 IPC 跳过 UI。

## 用前确认：CLI 是不是新版

新增 CLI 命令在 sibling `../tauri-pilot/` 仓库编译后**必须装到 PATH** 才能用——`pnpm dev:with-pilot` 起项目**不会**编译 CLI。

```bash
# 看 PATH 上的 tauri-pilot 跟仓库新代码同步否
tauri-pilot aijia --help | grep -c '^  [a-z]'   # 当前应是 35 条

# 装新版（每次 CLI 仓库代码有改动后跑一次）
cd ../tauri-pilot
cargo install --path crates/tauri-pilot-cli --force
```

跑 `aijia <verb>` 报 `unrecognized subcommand` 多半是这条没跑。

## ⏳ 待做

### 按场景列：跑测时被卡住的 CLI 缺口（2026-05-21 收口，按 `test-intents-cli-author` skill 铁则审计）

> **铁则**（再次强调）：每条 CLI = 一个原子 UI 动作（点一个按钮 / 填一个字段 / 等一个状态 / 读一段状态）；多步流程由 rules.md 串联，**不许写一体命令**；走 webview DOM eval，**不走 IPC**；selector 优先级 `data-aijia-*` > id > aria-label > textContent。

> **状态约定**：
> - ⏳ 真未做：CLI 缺口，给出**原子拆分清单 + selector 前置**
> - ⏳ 阻塞在 rules.md：CLI 已具备，问题在 rules 漂移或语义未定，应走 author skill
> - ❌ 不做：产品已废弃 / 环境依赖太重 / 不属于 UI 操作范畴。**给出 agent 退路**

#### 场景 F：日程编辑（日程 task 部分意图）— ⏳ 阻塞在 rules.md

`agenda-*` 11 个原子命令已全部落地。问题在 rules.md 跟当前 UI 命名漂移（"日程" → "定时任务"，"标题" 字段不存在，"组织者员工" → "负责员工"，"default 员工 record" 现实不存在等，详见 `run-reports/2026-05-20.md` Task 1）。

**agent 退路**：用 `test-intents-author` skill 重写日程 rules.md，按当前 UI 形态对齐。

#### 场景 H：崩溃恢复（崩溃恢复 task 全部）— ❌ 不属于 aijia CLI 范畴

**澄清**：`aijia` 子命令是"webview 内的 UI 操作打包器"。`pkill -9 + 重启 dev server + 等 socket` 是**操作系统进程级动作**，跟 webview DOM 毫无关系，硬塞进 aijia 命令会破坏"aijia = UI 原子"的语义边界。

**agent 退路**（在 runner skill / rules.md 里直接用 shell，建议沉淀进 `scripts/aijia-restart.sh`）：

```bash
# 1. 杀进程（只杀 aijia 子进程，不要 pkill aijia 它会连带杀父 cargo）
pkill -9 -f 'target/(debug|release)/aijia' || true

# 2. 等 socket 消失
while [ -S /tmp/tauri-pilot-com.aijia.app.sock ]; do sleep 0.2; done

# 3. 重启（用户另一个 terminal 已经在跑 pnpm dev:with-pilot，崩溃后它会自动 spawn）
# 或：cd lotus-app && pnpm dev:with-pilot &

# 4. 等 socket 回来
until [ -S /tmp/tauri-pilot-com.aijia.app.sock ]; do sleep 1; done

# 5. 等 webview ready
until aijia health-check --json 2>/dev/null | jq -e .ok > /dev/null; do sleep 1; done
```

这 5 步**不要**当 aijia 子命令做。原因：① kill 不是 UI 动作 ② socket 监控不是 UI 动作 ③ aijia CLI 在 socket 重建期间无法工作（封装它自己的重启很矛盾）。由 runner agent shell 直接跑。

#### 场景 J：人格管理（人格 task 全部）— ❌ 产品已废弃

memory `project_persona_deprecation_2026-05-10.md` 写明 persona 准备废弃，agenda 已改派 Employee，PR-5 完结。

**agent 退路**：
- 人格 task 的 rules.md 整组应该删除（走 author skill 走"deprecated"流程，迁移到 `archived-rules/` 目录）
- 不要花时间补 CLI；如果某条意图描述的能力被合并到了"数字员工"里，author 把它迁过去用 employee CLI

#### 场景 L：钉钉频道（钉钉频道 task 全部）— ❌ 环境依赖太重，建议人工跑

跑这个 task 需要：① 真实租户 ② 钉钉 AppKey/AppSecret/RobotCode ③ 一个能 @机器人的群 ④ 钉钉 webhook 回调能打到本机。**凭据不能 mock**（钉钉服务端会验签），网络回调也无法在 e2e 环境复现。

**agent 退路**：
- rules.md 钉钉 task 改为"semi-automated"标识，只跑 UI 路径（凭据用占位 string，落盘行为可以验，但发消息不会真打到钉钉）
- 真凭据 / 真消息这部分走人工 QA，每个 release 候选版人工 smoke test 一次（参考 release 流程文档）
- 不为这条 task 在 e2e 体系下补 CLI；如果将来 lotus 提供 sandbox 钉钉模拟器，再考虑

如果你只想测"填表保存"那一步：

```bash
# 用占位凭据，验证表单保存路径
aijia goto channel
# 后续 channel-* 原子命令同 team / settings 模式
```

selector 命名约定与 settings 一致：`data-aijia-channel-field={key}`、`data-aijia-channel-action="save"`。

#### 场景 M：设置面板填字段（设置 task 全部）— ❌ 不做（产品无此功能）

**2026-05-21 复核结论**：grep `src/components/settings/` 0 命中 `model` / `apiKey` / `baseUrl`。实际的 SettingsMenu 7 个 panel：

| key | 实际功能 |
|---|---|
| `account` | 退出登录、个人信息展示（GeneralPanel.tsx）|
| `account-billing` | personal 租户余额 / 月消耗 / 流水（AccountBillingPanel.tsx）|
| `archived` | 已归档会话列表（ArchivedPanel.tsx）|
| `runtime` | Node/Python/uv 诊断（RuntimePanel.tsx）|
| `about` | 版本信息（AboutPanel.tsx）|
| `usage` / `permissions` / `mcp` / `sso` / `shortcuts` | `disabled: true`，UI 占位不可点 |

理由：lotus-app 的 LLM 走 lotus gateway（OpenAI 兼容协议），凭据由租户后台配置，**桌面端用户不暴露 API key / 模型切换 UI**（参见 CLAUDE.md「lotus LLM 接入与 model id 路由现状」）。

**agent 退路**：
- rules.md 设置 task "改 API key / 切模型" 等意图是**对产品的错误假设**，应整组删除或迁档（走 author skill）
- 设置面板能验的事很少：① 退出登录 → 已有 `aijia logout` ② 计费数据展示 → 用 `aijia eval` 读 DOM（read-only 兜底，违反铁则但仅限读）③ 已归档会话恢复 → 如果真要测，补一条 `aijia archived-restore --id <s>`（冷门，等真用时再补）
- **不要**为不存在的功能补 CLI

> 历史教训沉淀：写场景待补清单时**先用 grep 验证产品功能是否存在**，再列 CLI。本场景之前列了 `settings-fill --field model.apiKey` 是凭设计想象写的，产品根本没这个字段。同类错误以前在 rules.md 漂移问题里也出现过（"日程" → "定时任务"等）。

#### 场景 N：待办队列（待办队列 task 全部）— ⏳ 阻塞在 rules + 产品语义

CLI 端无明显缺口（聊天流复用 `type-message / send / wait-reply / where`）。**阻塞在 pending 触发条件不明确**：
- HANDOFF：实测找不到 pending 触发方式
- `session_id ↔ conv_id` 命名混乱（memory `project_intent_test_gaps_2026-05-20.md`）

**agent 退路**：
1. author skill 先找产品 owner 对齐 `enqueue_or_send` 的"忙判定"信号（streaming 中？toolCall 中？AgentIdle 之前？）
2. 对齐后看是否需要 mock provider（让流式可控）来稳定复现 pending 入队
3. 如果产品认为"用户连发"应该走前端 dedup / 节流而不入队，rules 整组应该改语义（pending 不是用户视角的概念）
4. 都没着落前，把 task 标 `BLOCKED-PENDING-PRODUCT`，agent 跳过

---

### 已实现但需要修复的 CLI 行为问题

仅列「现成 CLI 有 bug / 不准」的，不重复列上面待补的清单。

| CLI | 问题 | 修复方向 |
|---|---|---|
| `aijia wait-reply` | 工具链长 turn 中提前误判停止（详见下方独立段） | 改监听 `AgentIdle` 事件而非只看 `isStreaming` |
| `aijia login` 错密码 | 已修一半（不再误报 logged_in），但 reason 仍 `timeout` 而非 `login_failed` —— 根因在产品 `isAuthPending` 不复位 | 产品方先修 `isAuthPending`，CLI 再改判定 |
| `aijia screenshot` | `html-to-image` 序列化 + 并发 eval 时报 `eval timed out`（runner skill §5.5）| 用 `tauri-pilot screenshot <path>` raw 命令兜底 |

---

## ⚠️ `aijia wait-reply` 在工具链长 turn 中提前误判停止（2026-05-21）

**问题**：跑数字员工 task 意图 3 时，dispatch 后 AI 走完整工具链（`Skill → SearchMemory + 4×WebSearch → Write → WriteMemory`，共 9 个 tool_use turn）。`wait-reply --timeout 120` 在 dispatch 之后 ~10s 内就返 `{ok:true, stableTicks:3, streaming:false}`。但 chatStore 此时只有 1 条 user，**0 条 assistant text**——AI 实际还在跑工具调用阶段。

**症状识别**：
- `aijia ui-message --include-tools` 只返 1 条 user，0 条 assistant
- `aijia last-reply` 返 None / 报 NoneType error
- 截图看 UI 仍在显示"思考中..."、tool bubble 还在 spinner
- 磁盘 `messages.jsonl` 看到多条 `role: assistant, content.text: ""` 但 `toolCalls: [...]` 非空 —— assistant 在做 tool_use only turn，正文还没出

**根因**：`wait-reply` stability window 只看 `isStreaming === false` 的 3 连续 ticks。但 AI 走多步工具链时，每步 tool_use 完成 → 等 tool_result → 下次 tool_use 之间，`isStreaming` 会瞬时为 false 多个 tick；这些"工具间隙"被误判为 turn 结束。

**正确语义**：
- 真完成应该是 `streamDone` 事件触发后、且**没有**待 ack 的 tool_use；或者更准确：`AgentIdle` 事件后才算 turn 完成
- runner skill §5.2 已写「3 连续 ready tick」但没区分"工具间隙 vs 真 idle"——需要升级

**临时绕路**（agent 跑测时手动用）：

```bash
# wait-reply 返回后再等一阵看 messageCount 是否还在增长
prev=$(aijia where --json | jq -r .messageCount)
sleep 30
cur=$(aijia where --json | jq -r .messageCount)
[ "$cur" = "$prev" ] || echo "AI 仍在跑，messageCount $prev → $cur"
```

**修复方向**：
- `wait-reply` 应监听 `AgentIdle` 事件（runtime 已发，TauriEventAdapter 已映射）而不是只看 isStreaming
- 或者新增 `aijia wait-agent-idle [--timeout 300]` 区分两种语义；当前 wait-reply 保留给"单 turn 简单回复"场景

实战出处：数字员工 task 意图 3，2026-05-21 09:50。dispatch → wait-reply 10s 就返 ok，实际 AI 在工具链跑了 2 分钟才真出 text。



**conv.json 落盘正确**：标题被 AI 提取为 "E2E上传测试文件内"（截断了"容"），说明前置链路是真在跑、LLM 真在用 attachment 内容。

### `aijia search --query <q> --scope local|global` — P2 待 UI

- 当前 grep 没找到 search 面板组件 / `Cmd+K` 入口；可能未实现
- 待 lotus-app 加搜索 UI 后再封 CLI

## 与现有清单的关系

本批补完后，runner skill §4.2 的命令清单从 16 增至 ~35（含原子化拆分），13 个 task 中除 upload / search / 钉钉外的意图主路径都能用原子命令序列覆盖。

## 设计原则（这次实战沉淀，与 `.claude/skills/test-intents-cli-author/SKILL.md` 一致）

1. **原子 + 组装**：一条 CLI 做一个原子 UI 动作；多步流程由 rules.md 串
2. **selector 优先级**：`data-aijia-*` > id > aria-label > textContent
3. **每个可被 CLI 操作的元素，lotus-app 侧加 `data-aijia-<noun>`**（详见 skill 的命名约定表）
4. **永远不能模拟"靠 React store 推 state"绕过 UI**——CLI 是 UI 操作打包器，不是 IPC / store 替身
5. **失败信号必须结构化**：`{ok: bool, reason?: snake_case, ...诊断字段}`，不抛非零退出码（除 socket 断连等 infra 故障）

## ⚠️ rules.md 不应让 agent 删盘上数据来"满足前提"（2026-05-20）

**问题**：登录 task `rules.md` 多条意图（1 / 2 / 3 / 5）的「前提」段写：

> `~/.renlijia/global/auth/active_account.json` 在测试前不存在（或先手动删除以确保从 0 开始）

这违反 runner skill §6 环境契约「直接在真实 `~/.renlijia/` 跑、不隔离、跑后**不**清理」。如果 agent 严格按字面执行，会把当前登录账号的 active_account.json 删掉、甚至连带 `users/t_28__u_54/` 整个 scope 目录（含历史会话 / 项目记忆 / agenda / employees 等真实数据）一起清，**破坏现场**。

**正确做法**：

- 「从 0 开始」并不必须，登录这条意图测的是「登录后**写入**这一步」，验收应该用 **mtime 变化** + sha256 改变 + 登录后字段非空来判定，不依赖"测试前文件不存在"
- 类似「文件不存在」「目录为空」的前提语句，rules 作者**不应该写**——意图测试不准 sandbox / 不准清理，作者写了等于把��条意图变成"必须先破坏现场才能跑"
- 如果某条意图本质上要测"首次登录冷启路径"（即 active_account.json 真的从未写过的代码分支），那只有**新电脑 / 新用户**能复现，应该在 rules 里直接写明「本意图只在新电脑上跑，否则 SKIPPED」，而不是让 agent 删文件硬复现

**待 author 修复的范围**（rules.md 提级修订）：

| Task / 意图 | 违反前提 | 改写方向 |
|---|---|---|
| 登录 / 意图 1 | `active_account.json` 不存在 | 改为：记录登录前 mtime + sha256，验收登录后 mtime 改变 + 字段非空 |
| 登录 / 意图 2 | `active_account.json` 不存在 | 改为：记录登录前 mtime + sha256，验收登录失败后**未写入**（mtime 不变 + sha256 一致） |
| 登录 / 意图 3 | （隐含：登录态 → 登出后清理） | 现状 OK，但写「文件被删除或内容已被清空」要明确二选一，不要让 agent 现场猜 |
| 登录 / 意图 5 | `users/{scope}/brand.json` 不存在 | 改为：登录前若 brand.json 已存在则记录 mtime + 内容 hash，验收登录后写入 / 覆盖；或明确"非首次登录可 SKIPPED" |

**追加项**：runner skill §6 环境契约里补一条「rules 写的前提不准要求 agent 删除现有文件 / 目录；agent 看到这种前提应记录为 `FAIL 主因 = rules/CLI 问题` 并跳过破坏性步骤」。

实战出处：登录 task 意图 1，2026-05-20 跑测。用户原话「为什么要删除啊 我操了 这个肯定是 rule 有问题」。

