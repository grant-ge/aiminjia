# rules.md — Runtime

本 task 测的产品承诺：**AIjia 管理的本地 Runtime（Node / npm / Python / uv）在新设备首次安装后能自动下载并安装；对话里的本地命令默认注入 AIjia Runtime 环境；第三方包安装状态能跨新对话复用；用户明确要求系统环境时不会被悄悄切回 AIjia Runtime**。

UI 文案对应：设置里的「Runtime」面板，以及对话动态上下文中注入的 Runtime 环境说明。

## 测试范围

覆盖 Managed Runtime 的首次下载/安装、诊断、Bash / PowerShell 默认 Runtime 环境注入、显式系统环境出口、以及 npm / Python 包安装后的同一应用进程内新对话复用。关注 `src-tauri/src/runtime/dependencies/`、`src-tauri/src/runtime/chat/context_builder.rs`、`src-tauri/src/runtime/tools/builtin/bash.rs`、`src-tauri/src/runtime/tools/builtin/powershell.rs`、`src-tauri/src/runtime/tools/catalog.rs`、`src/components/settings/panels/RuntimePanel.tsx`。

本 task 的包复用意图只覆盖**同一应用进程内**的新对话复用。关闭应用后再打开是否仍复用，是跨进程持久化维度，后续应单独写意图；不能混在同一条里。

MCP stdio 默认 Runtime 环境注入先不写入本 task 的 L4 意图：只有在 `tauri-pilot aijia` 具备稳定的 MCP 配置、连接、调用原子命令后，才新增单独意图。Skill `!cmd` 当前没有稳定用户入口触发 shell 执行，先由 Rust 集成测试覆盖，不写入本 task 的 L4 意图。

## 真实用户话术矩阵

Runtime 意图不能只测用户明确说出 `managed runtime`、`npm prefix` 这类技术词。真实用户通常只描述任务、边界或担心点，Agent 需要从这些话术里推断该用默认 AIjia Runtime 还是系统环境。

| 话术类型 | 用户通常会说 | 期望链路 |
|---|---|---|
| 普通业务计算型 | “帮我算一下这段 CSV 的平均值/中位数” | 不提 Runtime 时默认用 AIjia managed runtime；见 `意图-Runtime-006` |
| 开发工具型 | “帮我格式化这段 JS，别在项目里生成乱七八糟的文件” | 缺 Node CLI 时安装到 AIjia managed runtime，不污染当前项目；见 `意图-Runtime-007` |
| 缺包补齐型 | “缺哪个库你自己补一下，别让我配置环境” | 裸 `python` / `node` / `uv` / `npm` 命中 managed runtime；见 `意图-Runtime-001`、`意图-Runtime-002` |
| 新对话追问型 | “刚才那个工具再用一次，已经有了就别重新装” | 新对话复用 managed runtime 里的包，不重复安装；见 `意图-Runtime-001`、`意图-Runtime-002` |
| 系统环境型 | “这次用我电脑系统里装的 Node/Python” / “不要用你自带的” | 不传额外工具参数；使用动态上下文里的系统绝对路径，或先通过系统命令探测路径；见 `意图-Runtime-005`、`意图-Runtime-008` |
| 自带环境型 | “用你自己的环境跑一下” / “别动我电脑系统环境” | 不走 system 出口，默认裸命令命中 AIjia managed runtime；见 `意图-Runtime-009` |
| 默认来源诊断型 | “你现在本地命令到底从哪来的？” | 通过真实命令输出判断默认路径，不复述说明；见 `意图-Runtime-004` |
| 首次安装型 | “新电脑装好后 Runtime 面板有没有准备好？” | 首次启动后下载、解压、指针与设置页一致；见 `意图-Runtime-003` |

已安装技能或 dws 类企业 CLI 是否重复安装，归 `技能` task 覆盖；本 task 只覆盖 shell / MCP / Skill `!cmd` 这些 Runtime 环境入口。

MCP stdio / Skill `!cmd` 属于真实入口，但当前缺少稳定 `tauri-pilot aijia` 原子命令；本 task 只记录缺口，不写不可执行的 L4 意图。

## 通用判定口径

- `{scope}` 从 `tauri-pilot aijia where --json` 推断。
- `{runtime_root}` 按平台推断：
  - Windows：`%LOCALAPPDATA%\renlijia-runtimes\renlijia-primary-runtime`
  - macOS：`~/Library/Caches/renlijia-runtimes/renlijia-primary-runtime`
  - Linux：`~/.cache/renlijia-runtimes/renlijia-primary-runtime`
- `{current_pointer}` 是 `{runtime_root}/current` 的文本内容。
- `{active_runtime}` 是 `{runtime_root}/{current_pointer}`。
- `{managed_prefix}` 指 `npm config get prefix` 输出的路径，该路径以 `{runtime_root}` 开头；仅在意图明确要求 npm 安装位置时使用。
- `AIjia managed runtime 的 Node 命令目录` 指 PATH entry 规范化后以 `{active_runtime}/node` 开头。
- `AIjia managed runtime 的 Python 命令目录` 指 PATH entry 规范化后以 `{active_runtime}/python` 开头。
- `AIjia managed runtime 的 uv 命令目录` 指 PATH entry 规范化后以 `{active_runtime}/uv` 开头。
- `裸 node` / `裸 npm` / `裸 npx` / `裸 prettier` / `裸 uv` / `裸 uvx` / `裸 python3` / `裸 python` 指 `toolCalls[].arguments.command` 中的命令 token 是对应字面命令名，且该 token 前面不是 `/`、`\`、`.`、`~` 或盘符路径。
- `工具输出中的命令路径` 可以来自 `where` / `which` / `Get-Command` / `command -v` / `process.execPath` / `sys.executable` 等真实命令输出；不要求固定使用某一种命令。
- `系统环境请求` 指用户明确说“系统自带”“我电脑上的”“不要用你自带的”时，Agent 通过动态上下文里的系统绝对路径或系统命令探测结果调用对应命令；工具参数中不存在 `runtime_env`。
- `Python 安装命令数量` 统计 `toolCalls[].arguments.command` 同时包含目标包名和以下任一片段的工具调用数量：`pip install`、`python -m pip install`、`python3 -m pip install`、`uv pip install`。
- `Node 安装命令数量` 统计 `toolCalls[].arguments.command` 同时包含目标包名和以下任一片段的工具调用数量：`npm install`、`npm i`、`npm add`、`pnpm install`、`pnpm add`、`yarn add`、`npx `。
- transcript 路径是 `~/.renlijia/users/{scope}/conversations/{conv}/messages.jsonl`。
- `messages.jsonl` 解析时按 `\t✓\n` 分隔记录，再解析每条记录里的 JSON 前缀。
- 本 task 不删除 `{runtime_root}`，也不删除当前用户真实 conversation 历史。
- Windows 平台裸 Python 命令允许使用 `python`；macOS / Linux 平台裸 Python 命令允许使用 `python3`。除非 Runtime artifact 后续提供跨平台 `python` shim，否则 L4 意图不强制所有平台都能裸跑 `python`。

---

## 意图-Runtime-001: 装包完成后，新对话不重复安装

**场景**
用户在一个对话里让 AI 自己补齐缺失的 Python / Node 小工具，不想手动配置环境，也不想污染电脑系统环境。安装完成或确认已可用后，用户新建另一个对话再次使用同样的包，AI 直接复用本地 Runtime 里的已安装包，只做可用性检查，不重复执行安装命令。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 `{scope}`、`{runtime_root}`、`{current_pointer}`、`{active_runtime}`
3. 轮 A（允许补齐本地包）：
   1. 新建空对话：`tauri-pilot aijia new-task`
   2. 输入：
      ```text
      我想做个小演示：把数字 1234567 变成英文的人类可读写法，再用 cowsay 输出一句 runtime-ok。
      缺哪个本地小工具你自己补一下，不要让我配置环境，也不要动我电脑系统里的 Node 或 Python。
      为了我确认你没有写死路径，请把本次 Python 和 Node 的可执行文件路径也输出出来。
      ```
   3. `tauri-pilot aijia send`
   4. `tauri-pilot aijia wait-reply --timeout 300`
   5. 记下 `conv_a=$(tauri-pilot aijia where --json | jq -r .activeConversationId)`
4. 轮 B（新对话复用本地包）：
   1. 新建空对话：`tauri-pilot aijia new-task`
   2. 输入：
      ```text
      刚才那个 humanize 和 cowsay 再用一次：把 7654321 转成人类可读写法，再让 cowsay 输出 runtime-reuse。
      如果它们已经在你自己的环境里可用了，就直接复用，不要重新安装，也不要用我电脑系统环境。
      不要写死本机路径，不要用 npx 临时下载。
      ```
   3. `tauri-pilot aijia send`
   4. `tauri-pilot aijia wait-reply --timeout 180`
   5. 记下 `conv_b=$(tauri-pilot aijia where --json | jq -r .activeConversationId)`
5. 打开 `~/.renlijia/users/{scope}/conversations/{conv_a}/messages.jsonl`
6. 打开 `~/.renlijia/users/{scope}/conversations/{conv_b}/messages.jsonl`

**验收标准**
- 轮 A 的 `messages.jsonl` 中存在 `toolCalls[].name == "Bash"` 或 `toolCalls[].name == "PowerShell"` 的记录
- 轮 A 的工具调用命令包含裸 `node`
- 轮 A 的工具调用命令包含裸 `npm`
- 轮 A 的工具调用命令包含裸 `python3` 或裸 `python`
- 如果轮 A 执行 Python 安装命令，该命令包含裸 `uv`
- 轮 A 的工具调用命令不包含 `{active_runtime}` 下的 Node 可执行文件绝对路径
- 轮 A 的工具调用命令不包含 `{active_runtime}` 下的 Python 可执行文件绝对路径
- 轮 A 的工具输出文本包含 `humanize.intword(1234567)` 的结果 `1.2 million`
- 轮 A 的工具输出文本包含字面值 `runtime-ok`
- 轮 A 的 Python `sys.executable` 输出路径位于 `{active_runtime}` 下
- 轮 A 的 Node `process.execPath` 输出路径位于 `{active_runtime}` 下
- 轮 A 的 `npm config get prefix` 输出路径位于 `{managed_prefix}` 下
- 轮 B 的 `messages.jsonl` 中存在 `toolCalls[].name == "Bash"` 或 `toolCalls[].name == "PowerShell"` 的记录
- 轮 B 的工具调用命令包含 `humanize`
- 轮 B 的工具调用命令包含 `cowsay`
- 轮 B 的 Python 安装命令数量 `== 0`
- 轮 B 的 Node 安装命令数量 `== 0`
- 轮 B 的工具调用命令不包含 `npm install`、`npm i`、`npm add`、`pnpm install`、`pnpm add`、`yarn add`、`pip install`、`uv pip install`
- 轮 B 的工具输出文本包含 `7.7 million`
- 轮 B 的工具输出文本包含字面值 `runtime-reuse`
- 轮 B 的工具输出文本不包含 `ModuleNotFoundError`
- 轮 B 的工具输出文本不包含 `Cannot find module`
- 轮 B 的工具输出文本不包含 `command not found`
- 轮 B 的工具输出文本不包含 `npm error`
- 轮 B 的工具输出文本不包含 `npm ERR!`
- 轮 B 的最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 不为空
- 跑完轮 B 后 `tauri-pilot aijia health-check` 返回 ok

---

## 意图-Runtime-002: 默认包路径检查后，新对话复用包

**场景**
用户让 AI 检查并使用一组普通 Python / Node 依赖，表达的是“能用就直接用，缺了你自己补”，不是让 AI 切到系统全局环境。新开对话再次使用同样的包时，AI 通过当前 Runtime 环境检查包是否已存在；如果已存在，只复用已安装包，不走系统全局环境检查，也不重复下载或安装。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 `{scope}`、`{runtime_root}`、`{current_pointer}`、`{active_runtime}`
3. 轮 A（允许补齐本地包）：
   1. 新建空对话：`tauri-pilot aijia new-task`
   2. 输入：
      ```text
      帮我确认两个小依赖能不能直接用：Python 的 humanfriendly 和 Node 的 is-number。
      能用就跑一下：humanfriendly 格式化 1536 字节，is-number 判断 42。
      如果缺了，你自己补到你可用的环境里；不要让我配系统 PATH，不要写死本机路径，不要用 npx 临时下载。
      ```
   3. `tauri-pilot aijia send`
   4. `tauri-pilot aijia wait-reply --timeout 300`
   5. 记下 `conv_a=$(tauri-pilot aijia where --json | jq -r .activeConversationId)`
4. 轮 B（新对话复用本地包）：
   1. 新建空对话：`tauri-pilot aijia new-task`
   2. 输入：
      ```text
      上次那两个依赖 humanfriendly 和 is-number 再帮我用一下。
      如果已经能用了，就别重新安装；humanfriendly 格式化 2048 字节，is-number 判断字符串 "2048"。
      不要手动拼 NODE_PATH，不要查系统全局包，也不要用 npx。
      ```
   3. `tauri-pilot aijia send`
   4. `tauri-pilot aijia wait-reply --timeout 180`
   5. 记下 `conv_b=$(tauri-pilot aijia where --json | jq -r .activeConversationId)`
5. 打开 `~/.renlijia/users/{scope}/conversations/{conv_a}/messages.jsonl`
6. 打开 `~/.renlijia/users/{scope}/conversations/{conv_b}/messages.jsonl`

**验收标准**
- 轮 A 的 `messages.jsonl` 中存在 `toolCalls[].name == "Bash"` 或 `toolCalls[].name == "PowerShell"` 的记录
- 轮 A 的工具调用命令包含 `humanfriendly`
- 轮 A 的工具调用命令包含 `is-number`
- 轮 A 的工具调用命令包含裸 `python3` 或裸 `python`
- 轮 A 的工具调用命令包含裸 `node`
- 轮 A 的工具调用命令不包含 `{active_runtime}` 下的 Node 可执行文件绝对路径
- 轮 A 的工具调用命令不包含 `{active_runtime}` 下的 Python 可执行文件绝对路径
- 轮 B 的 `messages.jsonl` 中存在 `toolCalls[].name == "Bash"` 或 `toolCalls[].name == "PowerShell"` 的记录
- 轮 B 的工具调用命令包含 `humanfriendly`
- 轮 B 的工具调用命令包含 `find_spec("humanfriendly")`
- 轮 B 的工具调用命令包含 `is-number`
- 轮 B 的工具调用命令包含 `require("is-number")`
- 轮 B 的工具调用命令不包含 `NODE_PATH=`
- 轮 B 的工具调用命令不包含 `$env:NODE_PATH`
- 轮 B 的工具调用命令不包含 `npx`
- 轮 B 的工具调用命令不包含 `command -v`
- 轮 B 的工具调用命令不包含 `Get-Command`
- 轮 B 的工具调用命令不包含 `where is-number`
- 轮 B 的工具调用命令不包含 `npm list -g`
- 轮 B 的工具调用命令不包含 `pip show humanfriendly`
- 轮 B 的工具调用命令不包含 `pip list`
- 轮 B 的 Python 安装命令数量 `== 0`
- 轮 B 的 Node 安装命令数量 `== 0`
- 轮 B 的工具调用命令不包含 `npm install`、`npm i`、`npm add`、`pnpm install`、`pnpm add`、`yarn add`、`pip install`、`uv pip install`
- 轮 B 的工具输出文本包含 `2 KiB` 或 `2.05 KB`
- 轮 B 的工具输出文本包含 `true`
- 轮 B 的工具输出文本不包含 `ModuleNotFoundError`
- 轮 B 的工具输出文本不包含 `Cannot find module`
- 轮 B 的工具输出文本不包含 `command not found`
- 轮 B 的工具输出文本不包含 `npm error`
- 轮 B 的工具输出文本不包含 `npm ERR!`
- 跑完轮 B 后 `tauri-pilot aijia health-check` 返回 ok

---

## 意图-Runtime-003: 首次安装后，环境下载完成

**场景**
用户在一台从未运行过 AIjia 的新电脑或全新系统用户里安装 AIjia。首次启动后，应用从默认运行期 manifest 下载 Runtime 压缩包，解压出 Node / Python / uv，并写入当前版本指针；不能把旧机器 cache 或 bundled fallback 当作首次下载结果。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 记录 `T0`；本意图只允许在刚完成干净安装并首次启动的测试机器或全新系统用户中执行
3. 推断 `{runtime_root}`
4. 从测试记录确认首次启动前 `{runtime_root}/current` 不存在；如果首次启动前已经存在，本意图记为 SKIPPED，换新电脑或全新系统用户重跑，不要删除当前用户的 runtime 目录硬造环境
5. 打开设置：`tauri-pilot aijia open-settings`
6. 等设置弹窗出现：`tauri-pilot aijia settings-wait`
7. 切到「Runtime」面板：`tauri-pilot aijia settings-select-panel --key runtime`
8. 等待「Runtime」面板不再显示加载态，并能看到 Node / Python / uv 诊断信息
9. 检查 `{runtime_root}/downloads/`
10. 读取 `{runtime_root}/current` 的文本内容，记为 `{current_pointer}`
11. 从 `{current_pointer}` 中去掉 `versions/` 前缀，记为 `{bundle_version}`
12. 检查 `{runtime_root}/{current_pointer}`，记为 `{active_runtime}`
13. 使用 `{active_runtime}` 下的 Node / Python / uv 可执行文件分别运行 `--version`
14. 检查 `~/.renlijia/logs/renlijia.log`

**验收标准**
- 文件 `{runtime_root}/current` 存在
- `{runtime_root}/current` 的文本内容不为空
- `{current_pointer}` 以 `versions/` 开头
- 目录 `{active_runtime}` 存在
- 目录 `{runtime_root}/downloads/` 存在
- `{runtime_root}/downloads/` 下存在文件名包含 `renlijia-primary-runtime` 的压缩包
- 该压缩包的 mtime 在 `T0 ± 5 分钟` 内
- 该压缩包的 size `> 0`
- 目录 `{active_runtime}/node/` 存在
- 目录 `{active_runtime}/python/` 存在
- 目录 `{active_runtime}/uv/` 存在
- 使用 `{active_runtime}` 下的 Node 可执行文件运行 `--version` 时，stdout 不为空
- 使用 `{active_runtime}` 下的 Python 可执行文件运行 `--version` 时，stdout 不为空
- 使用 `{active_runtime}` 下的 uv 可执行文件运行 `--version` 时，stdout 不为空
- 设置页 Runtime 面板展示的 Node 路径位于 `{active_runtime}` 下
- 设置页 Runtime 面板展示的 Python 路径位于 `{active_runtime}` 下
- 设置页 Runtime 面板展示的 uv 路径位于 `{active_runtime}` 下
- `~/.renlijia/logs/renlijia.log` 中存在 `cache runtime initialized version={bundle_version}`
- 测试记录中首次启动前 `{runtime_root}/current` 不存在
- `~/.renlijia/logs/renlijia.log` 中不存在 `manifest ensure failed; trying bundled fallback`
- `~/.renlijia/logs/renlijia.log` 中不存在 `bundled fallback install start`
- 任一版本命令 stdout 不为空
- 任一检查步骤输出不包含 `checksum`
- 任一检查步骤输出不包含 `extract`
- 任一检查步骤输出不包含 `smoke test`
- 任一检查步骤输出不包含 `network error`

---

## 意图-Runtime-004: 默认命令环境，命中内置环境

**场景**
用户让 AI 判断当前本地命令环境来自哪里。AI 要通过真实命令输出完成判断，而不是复述系统说明；在默认工具环境下，裸命令会命中 AIjia managed runtime。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 `{scope}`、`{runtime_root}`、`{current_pointer}`、`{active_runtime}`
3. 新建空对话：`tauri-pilot aijia new-task`
4. 输入：
   ```text
   帮我实际检查一下当前默认命令环境来自哪里，不要只根据上下文说明回答。
   请运行必要的本地命令来判断 node、npm、npx、Python、uv、uvx 是否来自 AIjia Runtime。
   最后用简短文字告诉我：这些默认命令是不是命中了 AIjia Runtime，并列出你依据的路径。
   不要手写任何 Runtime 绝对路径。
   ```
5. `tauri-pilot aijia send`
6. `tauri-pilot aijia wait-reply --timeout 180`
7. 记录 `conv=$(tauri-pilot aijia where --json | jq -r .activeConversationId)`
8. 打开 `~/.renlijia/users/{scope}/conversations/{conv}/messages.jsonl`

**验收标准**
- `messages.jsonl` 中存在 `toolCalls[].name == "Bash"` 或 `toolCalls[].name == "PowerShell"` 的记录
- 本轮对话的工具调用命令集合包含裸 `node`
- 本轮对话的工具调用命令集合包含裸 `npm`
- 本轮对话的工具调用命令集合包含裸 `npx`
- 本轮对话的工具调用命令集合包含裸 `uv`
- 本轮对话的工具调用命令集合包含裸 `uvx`
- Windows 平台本轮对话的工具调用命令集合包含裸 `python`
- macOS / Linux 平台本轮对话的工具调用命令集合包含裸 `python3`
- 本轮对话的工具调用命令集合不包含 `{active_runtime}` 下的 Node 可执行文件绝对路径
- 本轮对话的工具调用命令集合不包含 `{active_runtime}` 下的 Python 可执行文件绝对路径
- 工具输出中的 `node` 命令路径位于 `{active_runtime}` 下
- 工具输出中的 `npm` 命令路径位于 `{active_runtime}` 下
- 工具输出中的 `npx` 命令路径位于 `{active_runtime}` 下
- Windows 平台工具输出中的 `python` 命令路径位于 `{active_runtime}` 下
- macOS / Linux 平台工具输出中的 `python3` 命令路径位于 `{active_runtime}` 下
- 工具输出中的 `uv` 命令路径位于 `{active_runtime}` 下
- 工具输出中的 `uvx` 命令路径位于 `{active_runtime}` 下
- 工具输出不包含 `command not found`
- 工具输出不包含 `node: command not found`
- 工具输出不包含 `python: command not found`
- 工具输出不包含 `python3: command not found`
- 工具输出不包含 `uv: command not found`
- 工具输出不包含 `uvx: command not found`
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `AIjia Runtime`
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `{active_runtime}` 或工具输出里已出现的 Runtime 路径片段

---

## 意图-Runtime-005: 指定系统环境，不注入内置环境

**场景**
用户明确要求使用系统 Node / Python 环境。AI 要把“系统环境”和“AIjia Runtime”区分开：系统环境不可用时要说明不可用，不能悄悄改回 managed runtime。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 `{scope}`、`{runtime_root}`、`{current_pointer}`、`{active_runtime}`
3. 新建空对话：`tauri-pilot aijia new-task`
4. 输入：
   ```text
   这次请不要使用 AIjia Runtime。请实际检查系统环境里的 Node 和 Python。
   Windows 上请检查系统 node 和 python；macOS 或 Linux 上请检查系统 node 和 python3。
   如果系统 PATH 里没有对应命令，请直接说明系统环境不可用，不要改用 AIjia Runtime。
   最后用简短文字告诉我：系统环境是否可用，以及你看到的可执行路径。
   ```
5. `tauri-pilot aijia send`
6. `tauri-pilot aijia wait-reply --timeout 180`
7. 记录 `conv=$(tauri-pilot aijia where --json | jq -r .activeConversationId)`
8. 打开 `~/.renlijia/users/{scope}/conversations/{conv}/messages.jsonl`

**验收标准**
- `messages.jsonl` 中存在 `toolCalls[].name == "Bash"` 或 `toolCalls[].name == "PowerShell"` 的记录
- 本轮对话不存在 `toolCalls[].arguments.runtime_env` 字段
- 如果本轮工具调用直接执行 Node 或 Python 版本检查，命令中的 Node / Python 可执行文件使用动态上下文里的系统绝对路径，或命令先执行 `where` / `which` / `Get-Command` / `command -v` 等系统路径探测
- 如果工具输出包含 Node 可执行路径，该路径不位于 `{active_runtime}` 下
- 如果工具输出包含 Python 可执行路径，该路径不位于 `{active_runtime}` 下
- 如果系统 Node 不可用，最终 assistant 文本包含“系统环境不可用”
- 如果系统 Python 不可用，最终 assistant 文本包含“系统环境不可用”
- 工具输出不包含 `{active_runtime}` 下的 Node 可执行文件路径
- 工具输出不包含 `{active_runtime}` 下的 Python 可执行文件路径
- 工具输出不包含 `{active_runtime}` 下的 uv 可执行文件路径
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 不为空
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中不把 `{active_runtime}` 描述为本次系统环境路径

---

## 意图-Runtime-006: 用户分析 CSV，Python 命中内置环境

**场景**
用户贴一段 CSV 数据，让 AI 做一次普通数据分析。用户不提 Runtime、不提 PATH，也不指定 Python 绝对路径；AI 在需要本地计算时使用默认命令环境，裸 Python 命中 AIjia managed runtime。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 `{scope}`、`{runtime_root}`、`{current_pointer}`、`{active_runtime}`
3. 新建空对话：`tauri-pilot aijia new-task`
4. 输入：
   ```text
   我有一小段 CSV，帮我算每个部门的平均薪资、最高薪资员工，以及全表薪资中位数。
   如果你需要本地计算，请直接用你默认可用的 Python 命令运行，不要手写任何 Runtime 绝对路径。
   为了我核对环境来源，请在最终回答里附上本次 Python 可执行文件路径。

   name,department,salary
   Alice,Sales,12000
   Bob,Sales,15000
   Chen,Engineering,22000
   Dana,Engineering,26000
   Evan,Support,9000
   Fiona,Support,11000
   ```
5. `tauri-pilot aijia send`
6. `tauri-pilot aijia wait-reply --timeout 180`
7. 记录 `conv=$(tauri-pilot aijia where --json | jq -r .activeConversationId)`
8. 打开 `~/.renlijia/users/{scope}/conversations/{conv}/messages.jsonl`

**验收标准**
- `messages.jsonl` 中存在 `toolCalls[].name == "Bash"` 或 `toolCalls[].name == "PowerShell"` 的记录
- Windows 平台本轮对话的工具调用命令集合包含裸 `python`
- macOS / Linux 平台本轮对话的工具调用命令集合包含裸 `python3`
- 本轮对话的工具调用命令集合不包含 `{active_runtime}` 下的 Python 可执行文件绝对路径
- 工具输出中的 Python 可执行路径位于 `{active_runtime}` 下
- 本轮 `messages.jsonl` 中不存在 `toolCalls[].name == "Write"` 且 `toolCalls[].arguments.file_path` 指向用户桌面、用户文档、当前项目目录或当前业务工作目录下的 `.py` 文件
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `Sales`
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `13500` 或 `13,500`
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `Bob`
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `15000` 或 `15,000`
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `Engineering`
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `24000` 或 `24,000`
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `Dana`
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `26000` 或 `26,000`
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `Support`
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `10000` 或 `10,000`
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `Fiona`
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `11000` 或 `11,000`
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `13500` 或 `13,500`
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `{active_runtime}` 或工具输出里已出现的 Runtime 路径片段
- 工具输出不包含 `command not found`
- 工具输出不包含 `ModuleNotFoundError`
- 工具输出不包含 `SyntaxError`
- 工具输出不包含 `Traceback`
- 跑完本轮后 `tauri-pilot aijia health-check` 返回 ok

---

## 意图-Runtime-007: 用户格式化 JS，Node 包装进内置环境

**场景**
用户让 AI 使用常见 Node CLI 工具格式化一段 JavaScript。用户不关心 Runtime 细节，只要求不要污染当前项目；AI 缺包时把 CLI 安装到 AIjia managed runtime 的 npm prefix，并用裸命令执行。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 `{scope}`、`{runtime_root}`、`{current_pointer}`、`{active_runtime}`
3. 新建空对话：`tauri-pilot aijia new-task`
4. 输入：
   ```text
   请用本地 prettier CLI 帮我格式化下面这段 JavaScript，把格式化后的代码直接贴回消息里。
   如果 prettier 不可用，你可以自行补齐这个工具，但不要在当前项目目录里生成 node_modules 或 package.json。
   不要手写任何 Runtime 绝对路径。
   为了我核对环境来源，请在最终回答里附上 prettier 命令路径或 npm prefix。

   const user={name:"Ada",skills:["math","computing"]};function hello(x){return "hi, "+x.name}
   console.log(hello(user))
   ```
5. `tauri-pilot aijia send`
6. `tauri-pilot aijia wait-reply --timeout 300`
7. 记录 `conv=$(tauri-pilot aijia where --json | jq -r .activeConversationId)`
8. 打开 `~/.renlijia/users/{scope}/conversations/{conv}/messages.jsonl`

**验收标准**
- `messages.jsonl` 中存在 `toolCalls[].name == "Bash"` 或 `toolCalls[].name == "PowerShell"` 的记录
- 本轮对话的工具调用命令集合包含裸 `prettier`
- 如果本轮对话执行 Node 安装命令，该命令包含裸 `npm`
- 本轮对话的工具调用命令集合不包含 `npx`
- 本轮对话的工具调用命令集合不包含 `{active_runtime}` 下的 Node 可执行文件绝对路径
- 如果本轮对话执行 Node 安装命令，该命令不包含当前工作目录下的 `node_modules`
- 如果本轮对话执行 Node 安装命令，该命令不包含 `--prefix .`
- 如果本轮对话执行 Node 安装命令，该命令不包含 `pnpm add`
- 如果本轮对话执行 Node 安装命令，该命令不包含 `yarn add`
- 本轮 `messages.jsonl` 中不存在 `toolCalls[].name == "Write"` 且 `toolCalls[].arguments.file_path` 指向用户桌面、用户文档、当前项目目录或当前业务工作目录下的 `.js` 文件
- 如果工具调用命令包含 `prettier --write`，被写入的文件路径不位于用户桌面、用户文档、当前项目目录或当前业务工作目录
- 工具输出或最终 assistant 文本中的 prettier 命令路径位于 `{active_runtime}` 下，或 `npm config get prefix` 输出位于 `{managed_prefix}` 下
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `const user = {`
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `function hello(x)`
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `console.log(hello(user));`
- 工具输出不包含 `Cannot find module`
- 工具输出不包含 `command not found`
- 工具输出不包含 `npm ERR!`
- 跑完本轮后 `tauri-pilot aijia health-check` 返回 ok

---

## 意图-Runtime-008: 用户要求电脑环境，不回落内置环境

**场景**
用户用自然语言要求“用我电脑上的环境”。AI 使用动态上下文里的系统绝对路径，或先通过系统命令探测真实路径；系统命令不可用时给出不可用结论，不改用 AIjia managed runtime。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 `{scope}`、`{runtime_root}`、`{current_pointer}`、`{active_runtime}`
3. 新建空对话：`tauri-pilot aijia new-task`
4. 输入：
   ```text
   这次不要用你自带的运行环境，请用我电脑系统里安装的 Node 和 Python 看一下版本。
   Windows 上看 node 和 python；macOS 或 Linux 上看 node 和 python3。
   如果我电脑系统 PATH 里没有对应命令，就直接告诉我系统环境不可用，不要换成你自带的环境。
   ```
5. `tauri-pilot aijia send`
6. `tauri-pilot aijia wait-reply --timeout 180`
7. 记录 `conv=$(tauri-pilot aijia where --json | jq -r .activeConversationId)`
8. 打开 `~/.renlijia/users/{scope}/conversations/{conv}/messages.jsonl`

**验收标准**
- `messages.jsonl` 中存在 `toolCalls[].name == "Bash"` 或 `toolCalls[].name == "PowerShell"` 的记录
- 本轮对话不存在 `toolCalls[].arguments.runtime_env` 字段
- 如果本轮工具调用直接执行 Node 或 Python 版本检查，命令中的 Node / Python 可执行文件使用动态上下文里的系统绝对路径，或命令先执行 `where` / `which` / `Get-Command` / `command -v` 等系统路径探测
- 如果工具输出包含 Node 可执行路径，该路径不位于 `{active_runtime}` 下
- 如果工具输出包含 Python 可执行路径，该路径不位于 `{active_runtime}` 下
- 工具输出不包含 `{active_runtime}` 下的 Node 可执行文件路径
- 工具输出不包含 `{active_runtime}` 下的 Python 可执行文件路径
- 工具输出不包含 `{active_runtime}` 下的 uv 可执行文件路径
- 如果系统 Node 不可用，最终 assistant 文本包含“系统环境不可用”
- 如果系统 Python 不可用，最终 assistant 文本包含“系统环境不可用”
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 不为空

---

## 意图-Runtime-009: 用户要求自带环境，命中内置环境

**场景**
用户明确说“用你自带的运行环境”，不是默认模糊场景，也不是系统环境场景。AI 要把“AIjia 自带运行环境”和“电脑系统环境”区分开：在开关开启且 Runtime 可用时，裸 Node / Python 命令命中 AIjia managed runtime。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 `{scope}`、`{runtime_root}`、`{current_pointer}`、`{active_runtime}`
3. 新建空对话：`tauri-pilot aijia new-task`
4. 输入：
   ```text
   这次请明确使用你自带的 AIjia 运行环境，不要用我电脑系统 PATH 里的 Node 或 Python。
   请实际运行命令检查 Node 和 Python 的版本与可执行路径。
   Windows 上用 node 和 python；macOS 或 Linux 上用 node 和 python3。
   最后告诉我它们是否来自 AIjia 自带运行环境，并列出路径。
   不要手写任何 Runtime 绝对路径。
   ```
5. `tauri-pilot aijia send`
6. `tauri-pilot aijia wait-reply --timeout 180`
7. 记录 `conv=$(tauri-pilot aijia where --json | jq -r .activeConversationId)`
8. 打开 `~/.renlijia/users/{scope}/conversations/{conv}/messages.jsonl`

**验收标准**
- `messages.jsonl` 中存在 `toolCalls[].name == "Bash"` 或 `toolCalls[].name == "PowerShell"` 的记录
- 本轮对话的工具调用命令集合包含裸 `node`
- Windows 平台本轮对话的工具调用命令集合包含裸 `python`
- macOS / Linux 平台本轮对话的工具调用命令集合包含裸 `python3`
- 本轮对话不存在 `toolCalls[].arguments.runtime_env` 字段
- 本轮对话的工具调用命令集合不包含 `{active_runtime}` 下的 Node 可执行文件绝对路径
- 本轮对话的工具调用命令集合不包含 `{active_runtime}` 下的 Python 可执行文件绝对路径
- 工具输出中的 Node 可执行路径位于 `{active_runtime}` 下
- Windows 平台工具输出中的 Python 可执行路径位于 `{active_runtime}` 下
- macOS / Linux 平台工具输出中的 Python 可执行路径位于 `{active_runtime}` 下
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `AIjia`
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 包含 `{active_runtime}` 或工具输出里已出现的 Runtime 路径片段
- 工具输出不包含 `command not found`
- 工具输出不包含 `node: command not found`
- 工具输出不包含 `python: command not found`
- 工具输出不包含 `python3: command not found`
