---
name: test-intents-cli-author
description: Use when adding or modifying `aijia <verb>` CLI subcommands in tauri-pilot — wraps **single atomic** DOM operations (click one button / fill one field / wait one state) so intent-test agents can compose them into multi-step flows without burning tokens on snapshot+click loops. Triggers "加 aijia 子命令", "封装 X 流程到 CLI", "tauri-pilot 加 login", "扩 aijia CLI", "封装 UI 操作".
license: Internal
---

# test-intents-cli-author

## 这个 skill 干嘛的

给 AIjia 桌面应用补 `aijia <verb> <args>` 系列 CLI 子命令。每条命令 = **一个原子 UI 操作**（点一个按钮 / 填一个字段 / 等一个状态变化）。复杂流程由 rules.md / runner 按需**串成命令序列**，**不**塞进一条大命令里。

## 真相源

`docs/test-intents/cli-gap.md` 是**唯一**的 CLI 缺口清单——由真实跑意图的 agent 暴露的 gap 沉淀而来。写新 CLI 前先读它，做完一条划掉一条。**不要**自己想象需要哪些命令。

## 铁则：原子 + 组装

```
✅ aijia goto schedules
✅ aijia agenda-open-new
✅ aijia agenda-fill --field title --value "早会提醒"
✅ aijia agenda-fill --field prompt --value "..."
✅ aijia agenda-set-frequency daily
✅ aijia agenda-save

❌ aijia agenda-new --title --prompt --freq --start-at --emp   ← 一体大命令
```

### 为什么不能写"一体命令"

- **断点失明**：一体命令 8 步中第 5 步失败，rules.md 只看到 `ok:false reason:save_button_disabled`，无法在第 3 步插断言或 dump 中间态
- **复用率为 0**：另一条意图想"建到一半就关掉验证草稿不会污染列表"，需要原子的 fill / open，不需要 save
- **覆盖率塌方**：13 个 task 每个都有 2-5 条意图，原子命令组合可达 50+ 种序列；一体命令做一个就废一个
- **token 经济性**：原子失败信号小，单次 ≤200 tokens；一体命令出错带 lastProbe 全树，单次 1-2k

### 拆分判据

一条 CLI 只能做**下面之一**：
- 点一个按钮（含等按钮可点）
- 填一个字段（含 React-controlled value setter + dispatch input/change event）
- 选一个选项（select / combobox）
- 等一个状态（element appears / disappears / attribute changes）
- 读一段状态（dump store / dump DOM 子树 / 返回某个属性值）
- 触发一个原子事件（press key / focus / blur）

如果你的命令实现里有**两个独立 click**，或者**先 fill 后 click**，**必须拆**。例外：原子动作天然不可拆（如"点按钮然后等 dialog 出现"，按钮点击 + 等待是同一动作的连续两面）。

### 序列组装在哪做

**不是 CLI 的事**。rules.md / runner agent 自己 shell pipe 或顺序调命令：

```bash
aijia goto schedules
aijia agenda-open-new
aijia agenda-fill --field title --value "测试"
aijia agenda-fill --field prompt --value "Prompt 内容"
aijia agenda-set-frequency once
aijia agenda-set-start-at "2026-05-20T15:00"
aijia agenda-set-employee --name "小研"
aijia agenda-save
aijia agenda-wait-row --title "测试" --timeout 5
```

每行都能独立失败、独立断言、独立重试。

### `aijia login` 算原子吗

算。它做的是"提交登录表单"这一个用户动作——fill 两个字段 + 点按钮虽然技术上多步，但用户视角是一次"完成登录"。判断标准：**用户在 UI 上一次完成 / 一次失败的最小操作单元**。登录拆成 `fill account` + `fill password` + `click submit` 三步在用户视角没有��何意义（账号填一半的 UI 中间态没人验证）。

类似豁免：`dialog-click --action confirm` 是"做一次确认决定"。

## 底层一定走 eval 操作 DOM

- 跟现有 16 个子命令一致，**禁止**走 IPC
- 走 IPC 等于跳过 React 真实 mount/loading/校验/route guard，意图测试退化为单元测试
- 极少数例外（如 `archive-session` 走 `archive_conversation` IPC）必须写明原因 + 仅用于 setup / 清理，**不**用于"被测主流程"

## 为什么不让 agent 自己 click + eval

每点一步要 snapshot a11y 树定位元素 / 截图核对 / 再点；单次意图测试可烧 1-3 万 token，selector 还经常漂移。CLI 把"原子 UI 操作"做成稳定入口后，agent 只传入参读出参，token 降一个数量级。

## When to Use

- 读 `cli-gap.md` 看到 ❌ 没实现的条目
- 跑意图时 runner 报「FAIL 主因 = rules/CLI 问题，缺 `aijia <X>`」
- 用户说「加 aijia X 子命令」

## 先决条件

1. sibling clone 在位：从 lotus-app 仓库根看是 `../tauri-pilot/`
2. dev server 用 `pnpm dev:with-pilot`（带 `--features e2e`），不是 `pnpm tauri:dev`
3. dev build 暴露 `window.__aijia.{chatStore, sessionStore, authStore}`（见 `src/main.tsx`）

## `data-aijia-*` 命名约定（与现有代码对齐）

写 CLI 时第一步看现有 selector 能不能用，不行再加。**永远不要按 textContent 找按钮**（文案漂移成本极高）。

| 模式 | 例子 | 用途 |
|---|---|---|
| `data-aijia-<noun>`（无值） | `data-aijia-conversation-row` / `data-aijia-agenda-editor` / `data-aijia-employee-drawer` | 标识元素本身，CLI 用 `[data-aijia-<noun>]` 选 |
| `data-aijia-<noun>="<kind>"` | `data-aijia-dialog="permission-ask\|ask-user-question\|confirm"` | 标识元素本身，值是 kind 枚举（一组同名元素之间区分类型） |
| `data-aijia-<noun>-id={id}` / `-name={name}` / `-title={title}` | `data-aijia-conversation-id="conv-123"` / `data-aijia-employee-name="小研"` / `data-aijia-agenda-title="早会提醒"` | 派生标识符，CLI 用属性选择器精确定位 |
| `data-aijia-<noun>-action="<verb>"` | `data-aijia-dialog-action="confirm"` / `data-aijia-agenda-action="save"` / `data-aijia-employee-action="dispatch"` | 子元素角色枚举 |
| `data-aijia-<noun>-field="<name>"` | `data-aijia-agenda-field="title"` / `data-aijia-agenda-field="prompt"` | 表单字段标识 |
| `data-aijia-<noun>-status={enum}` / `-{prop}={value}` | `data-aijia-streaming="true"` / `data-aijia-agenda-status="active"` | 容器状态 |
| `data-aijia-nav={key}` | nav key（已定义 enum） | 导航入口 |

### 加 attr 的判据

**加**：
- textContent 会跟 i18n / 文案改动而变（侧栏 nav / 业务按钮文字）
- 同一页有多个语义相同的元素（如多个 row 的"立即运行"按钮，要按 row 定位）
- aria-label 缺失 / 不唯一

**不加**：
- 元素已经有稳定 `id` / 唯一 `aria-label`（如 LoginPage 的 `#account` `#password`）
- 通用 form 内的字段（有 placeholder + 框定到 form root 里即可）

### 不要把"e2e helper"塞回 lotus-app

❌ 错误：给 lotus-app 加 `window.__aijia.pushUploads(paths)` 直接往 zustand store 注入数据
原因：CLI 应该封装"用户能干的 UI 操作"，把数据塞 store 是绕过 UI = 退化为单测。这种情况标记 `cli-gap.md` 的对应条为"待真实 UI 路径方案"，**不要**用 helper 绕过。

唯一允许：dev-only 暴露 zustand store **只读 hook**（`window.__aijia.authStore` 用来 CLI dump scope）。**禁止**暴露 mutation helper。

### 例外：OS-level modal 是 e2e 死区，允许 mock"OS 之后的下一步"

如果某个 UI 路径调用了**操作系统级别**的 dialog / 权限弹窗（不在 webview 内，webview JS 无法操作），那条路径上"OS 那一步"物理上不可测。此时可以 dev-only 暴露 mock 队列让 CLI 注入"OS 完成后的数据"，但**下游真实代码必须照跑**。

判定是不是 OS-level：
- `@tauri-apps/plugin-dialog::open / save` —— ✅ OS-level（NSOpenPanel / Windows file dialog）
- `tauri-apps/plugin-dialog::ask / confirm / message` —— ✅ OS-level（系统对话框）
- 系统级权限授权（"允许 X 访问 ~/Documents"） —— ✅ OS-level
- Radix `<AlertDialog>` / `<Dialog>` / shadcn ConfirmDialog —— ❌ 不是 OS-level，是 DOM，**不许 mock**，必须真点
- 任何在 webview iframe / portal 里渲染的 modal —— ❌ 不是 OS-level

实施模式（已用于 `composer-click-plus` 案例）：
1. lotus-app `src/main.tsx` 加 `_pickAttachmentsMockQueue: [] as string[][]`（dev only）
2. 调 OS dialog 的 hook（如 `useChatAttachments.pickAttachments`）第一行加 `if (import.meta.env.DEV)` 检查队列，命中就消费、绕过 `open()`
3. **下游真实代码不动**——只跳过 `open()` 那一行
4. CLI 加两条原子：`<noun>-queue-...` 入队 + `<noun>-click-...` 点真实按钮触发 hook 读队列

判断是不是真的 OS-level：在 webview console 试 `document.querySelector(...)` 能不能选到 dialog 元素。能选到 = DOM modal（不许 mock），选不到 = OS-level（允许 mock）。

实战案例：`aijia composer-queue-files` + `aijia composer-click-plus`（OS file dialog mock）；`aijia dialog-click`（Radix DOM modal，真点不 mock）。

## 改动点（封装一条新 CLI 的完整路径）

### 1. `../tauri-pilot/crates/tauri-pilot-cli/src/cli.rs`

在 `enum AijiaCommand`（约 232 行起）加 variant：

```rust
/// Fill the agenda editor title field. Editor must already be open.
AgendaFillTitle {
    #[arg(long)]
    value: String,
},
```

约定：
- doc comment 第一行 = "封装的 UI 动作摘要 + 必要前置"
- enum variant CamelCase：`AgendaFillTitle` ↔ kebab CLI `agenda-fill-title`
- 必填用 `String`，可选用 `Option<String>`，开关用 `bool + default_value_t = false`

### 2. `aijia.rs::dispatch` 加 match 分支

```rust
AijiaCommand::AgendaFillTitle { value } => agenda_fill_title(client, &value, window).await,
```

### 3. 实现 async 函数（**核心**）

```rust
async fn agenda_fill_title(
    client: &mut Client,
    value: &str,
    window: Option<&str>,
) -> Result<Value> {
    // Single atomic op: fill title field inside agenda editor sheet.
    // Pre: editor must be open (data-aijia-agenda-editor present).
    let lit = js_string_literal(value);
    let script = format!(
        r#"(() => {{
            const root = document.querySelector('[data-aijia-agenda-editor]');
            if (!root) return {{ok: false, reason: 'editor_not_open'}};
            const el = root.querySelector('[data-aijia-agenda-field="title"]');
            if (!el) return {{ok: false, reason: 'title_field_missing'}};
            const proto = window.HTMLInputElement.prototype;
            const setter = Object.getOwnPropertyDescriptor(proto, 'value').set;
            setter.call(el, {v});
            el.dispatchEvent(new Event('input', {{bubbles: true}}));
            return {{ok: true, value: el.value}};
        }})()"#,
        v = lit,
    );
    eval_json(client, &script, window).await
}
```

### 4. 如果现有 selector 不够稳，在 lotus-app 加 `data-aijia-*`

按上一节命名约定加属性，**不要**自己造新模式。

## 标准 eval 模式

### 模式 A：点按钮（按 data-aijia-*-action）

```javascript
(() => {
  const root = document.querySelector('[data-aijia-<noun>]');
  if (!root) return {ok: false, reason: '<container>_not_open'};
  const btn = root.querySelector('[data-aijia-<noun>-action="<verb>"]');
  if (!btn) return {ok: false, reason: 'action_button_missing'};
  if (btn.disabled) return {ok: false, reason: 'action_button_disabled'};
  btn.click();
  return {ok: true};
})()
```

### 模式 B：fill React-controlled 文本输入框

```javascript
(() => {
  const el = document.querySelector('[data-aijia-<noun>-field="<name>"]');
  if (!el) return {ok: false, reason: 'field_missing'};
  const proto = el.tagName === 'TEXTAREA'
    ? window.HTMLTextAreaElement.prototype
    : window.HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, 'value').set;
  setter.call(el, <value>);
  el.dispatchEvent(new Event('input', {bubbles: true}));
  return {ok: true, value: el.value};
})()
```

**踩坑**：React-controlled `<input>` 不能直接 `el.value = x`，React 会覆盖。必须用 native setter + dispatch event。

### 模式 C：原生 `<select>`

```javascript
(() => {
  const sel = document.querySelector('select[aria-label="频率"]');
  if (!sel) return {ok: false, reason: 'select_missing'};
  const opt = [...sel.options].find(o => o.value === '<value>');
  if (!opt) return {ok: false, reason: 'option_missing'};
  const setter = Object.getOwnPropertyDescriptor(
    window.HTMLSelectElement.prototype, 'value'
  ).set;
  setter.call(sel, opt.value);
  sel.dispatchEvent(new Event('change', {bubbles: true}));
  return {ok: true};
})()
```

### 模式 D：Tiptap / contenteditable（已用于 `aijia type-message`）

```javascript
(() => {
  const ed = document.querySelector('.ProseMirror');
  if (!ed) return {ok: false, reason: 'editor_not_found'};
  ed.focus();
  document.execCommand('insertText', false, <literal>);
  return {ok: true};
})()
```

### 模式 E：Radix Select / combobox

shadcn/Radix Select 不是原生 `<select>`，要先点触发器再点选项 —— 这违反原子原则。**正确做法**：拆成两条 CLI：

```bash
aijia <noun>-open-<select>     # 点 trigger 打开 portal
aijia <noun>-pick-<value>      # 点指定 option
```

### 模式 F：等元素出现 / 消失（仅 wait_until helper 内部用）

```rust
async fn wait_until(
    client: &mut Client,
    probe_script: &str,
    timeout_ms: u64,
    window: Option<&str>,
) -> Result<Value>
```

probe script 返回 `{ready: bool, ...诊断}`。**只在原子动作"点 + 等 dialog 出现"这种连续两面**用 helper，不要用 helper 把多步 click 串起来。

### 模式 G：读 zustand store

```javascript
(() => {
  const cs = window.__aijia?.chatStore?.getState?.();
  if (!cs) return {ok: false, reason: 'store_missing'};
  return {ok: true, activeId: cs.activeConversationId, ...};
})()
```

## 出参约定

```jsonc
// 成功
{"ok": true, ...domain-specific fields...}

// 失败（结构化）
{"ok": false, "reason": "<short snake_case>", ...诊断字段}

// 超时
{"ok": false, "reason": "timeout", "timeoutSec": N, "lastProbe": {...}}
```

**禁止**直接 `return Err(...)` 让 CLI 进程退出非零码——逻辑失败（按钮没找到）和 infra 故障（socket 断）要让 runner 区分。

## 命名规范

- 子命令：`aijia <domain>-<atomic-verb>` —— domain 在前是因为后续会有多个 noun 各自一组原子操作
  - ✅ `aijia agenda-fill --field title --value "x"` / `aijia agenda-save` / `aijia agenda-set-frequency`
  - ✅ `aijia employee-open-card --name X` / `aijia employee-click-dispatch`
  - ❌ `aijia agenda-new`（一体大命令）
  - ❌ `aijia create-agenda`（动词在前不一致）
- enum variant：CamelCase 对应 kebab-case：`AgendaFillTitle` ↔ `agenda-fill-title`
- async fn：snake_case：`agenda_fill_title`

## 重编译 / 安装 quirks

1. **改 `bridge.js` 后 cargo 不重编**（`include_str!` 静态嵌入，`build.rs` 没 `cargo:rerun-if-changed`）。强制：`touch crates/tauri-plugin-pilot/src/lib.rs`
2. **改 `aijia.rs` / `cli.rs` 后必须重装 CLI 到 PATH**：`cargo build` 只产物到 `target/debug/`，不会替换 PATH 上的 `tauri-pilot`。其他 agent 跑 `tauri-pilot aijia <new-verb>` 还是老版找不到新命令。正确：
   ```bash
   # 从 lotus-app 仓库根
   cd ../tauri-pilot
   cargo install --path crates/tauri-pilot-cli --force
   ```
   这条把 release binary 装到 `~/.cargo/bin/tauri-pilot`，下次 shell 直接拿到。dev server 不用重启（CLI 是独立进程，每次调用时连 socket）。
3. **改 lotus-app 加 `data-aijia-*`**：HMR 自动刷新，app / CLI 都不用重启

## 测试新 CLI

```bash
# 从 lotus-app 仓库根
cd ../tauri-pilot
cargo install --path crates/tauri-pilot-cli --force   # 装到 PATH

# 另一终端：dev server（如果没在跑）
cd ../lotus-app && pnpm dev:with-pilot
until [ -S /tmp/tauri-pilot-com.aijia.app.sock ]; do sleep 2; done

# 确认 PATH 上的就是新版
tauri-pilot aijia --help | grep -c '^  [a-z]'          # 数下命令总数对不对

# 金路径
tauri-pilot aijia <new-verb> --arg ...

# 失败路径（前置条件不满足 / arg 不合法）
tauri-pilot aijia <new-verb> --arg <bad>

# 幂等性
tauri-pilot aijia <new-verb> --arg ...; tauri-pilot aijia <new-verb> --arg ...
```

## selector cheat sheet（持续累积，**先查这张表再去翻代码**）

| 元素 | selector | 来源 |
|---|---|---|
| 侧栏 nav 按钮 | `[data-aijia-nav="<key>"]` (key ∈ home/employees/expert-teams/skill-center/schedules/channel) | `src/components/sidebar/SidebarNav.tsx` |
| 侧栏「新任务」按钮 | `button` 中 `textContent.trim() === '新任务'` | aijia.rs new_task() |
| 会话行 | `[data-aijia-conversation-row]` + `[data-aijia-conversation-id="<id>"]` | `src/components/sidebar/ConversationRow.tsx` |
| Tiptap 编辑器 | `.ProseMirror` | LoginPage / Composer |
| 发送按钮 | `button[aria-label="发送"]` | aijia.rs send() |
| 停止按钮 | `button[aria-label="停止"]` | aijia.rs cancel() |
| 消息列表容器 | `[data-aijia-message-list]` | `MessageList.tsx` |
| AI 气泡 | `[data-aijia-ai-bubble][data-aijia-message-id="<id>"]` | `AiBubble.tsx` |
| 流式气泡 | `[data-aijia-streaming-bubble]` | `StreamingBubble.tsx` |
| Dialog 容器（三类合一） | `[data-aijia-dialog="permission-ask\|ask-user-question\|confirm"]` | `Modal.tsx` / `PermissionAskDialog.tsx` / `AskUserQuestionDialog.tsx` / `ConfirmDialog.tsx` |
| Dialog 标题 / 描述 | `[data-aijia-dialog-title]` / `[data-aijia-dialog-description]` | 同上 |
| Dialog 按钮 | `[data-aijia-dialog-action="allow\|deny\|cancel\|confirm\|option"]` | 同上 |
| AskUserQuestion option 定位 | `[data-aijia-dialog-question-index="N"][data-aijia-dialog-option-index="M"]` | `AskUserQuestionDialog.tsx` |
| LoginPage 账号 | `#account` | `LoginPage.tsx` |
| LoginPage 密码 | `#password` | `LoginPage.tsx` |
| LoginPage 提交 | `form (containing #account) button[type="submit"]` | `LoginPage.tsx` |
| 定时任务编辑器容器 | `[data-aijia-agenda-editor]` | `AgendaItemEditor.tsx` |
| 定时任务字段 | `[data-aijia-agenda-field="title\|prompt"]` + `#agenda-editor-start` (datetime) | `AgendaItemEditor.tsx` |
| 定时任务保存/取消 | `[data-aijia-agenda-action="save\|cancel"]` | `AgendaItemEditor.tsx` |
| 定时任务行 | `[data-aijia-agenda-row][data-aijia-agenda-title="<title>"]` + `data-aijia-agenda-id` / `data-aijia-agenda-status` | `ScheduleTaskRow.tsx` |
| 定时任务行操作按钮 | `button[aria-label^="立即运行 \|暂停 \|启用 \|取消 \|编辑 "]` | `ScheduleTaskRow.tsx` |
| 「+ 新建」 | `[data-aijia-agenda-new]` | `SchedulesPage.tsx` |
| 员工卡 | `[data-aijia-employee-card][data-aijia-employee-name="<name>"]` + `data-aijia-employee-id` | `EmployeeCard.tsx` |
| 员工 Drawer | `[data-aijia-employee-drawer]` | `EmployeeDrawer.tsx` |
| 员工派活按钮 | `[data-aijia-employee-action="dispatch"]` (drawer 内) | `EmployeeDrawer.tsx` |

新加 CLI 时发现的稳定 selector 都补到这张表。

## Red Flags（看到自己在这样想就停下来）

| 想法 | 应该 |
|---|---|
| 「这流程跨 5 个页面，写一个大命令省事」 | **拆成 5+ 条原子命令**——一体命令是这个 skill 最常踩的坑 |
| 「先 fill title 再 click save，反正都是新建流程」 | 拆。fill 和 save 是两个原子，意图可能只测 fill 不需要 save |
| 「按 textContent 找按钮够稳，不用加 data-attr」 | 文案会改 / 多个同名按钮会撞，加 `data-aijia-*-action` |
| 「Tauri webview 拦了 drop event，让我加个 `__aijia.pushUploads` helper」 | **不行**——绕过真实 UI = 不是意图测试。标 cli-gap.md "待真实 UI 路径" |
| 「OS file dialog 我直接 mock 整个 `pickAttachments` 返回吧」 | **不行**——只能 mock OS dialog 那一行 `open()` 调用，下游 `makePendingAttachment` / `insertAttachmentTokens` 等真实代码必须跑到 |
| 「失败直接 panic 让 CLI 退出非零码」 | 返回 `{ok: false, reason}`，runner 要分逻辑失败 vs infra 故障 |
| 「`el.value = x` 设 React-controlled 字段」 | 不行，React 同步覆盖。必须用 native setter + dispatch input event |
| 「我先跑两次手测验幂等，没问题就交付」 | 还要测：前置条件不满足时 reason 是不是结构化 |
| 「这个流程没有现有 CLI，让我顺手把整个 flow 写成一个命令」 | 停。先确认 cli-gap.md 是否列了这条；列了的话拆成它对应的原子命令 |
| 「这条命令出参只返回 ok: true 就够了」 | 返回足够的 domain id（agendaId / employeeId / conversationId）让序列后续步骤 / rules.md 验收用 |
