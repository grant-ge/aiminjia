# rules.md — Runtime

本 task 测的产品承诺：**AIjia 管理的本地 Runtime（Node / npm / Python / uv）在新设备首次安装后能自动下载并安装，对话工具中稳定可用，第三方包安装状态能跨新对话复用，不因为新建会话就重复下载或重复安装**。

UI 文案对应：设置里的「Runtime」面板，以及对话动态上下文中注入的 Runtime 工具路径。

## 测试范围

覆盖 Managed Runtime 的首次下载/安装、诊断、对话中 Runtime 路径注入、Bash 工具使用 Runtime Node/Python 执行命令、以及 npm / Python 包安装后的跨新对话复用。关注 `src-tauri/src/runtime/dependencies/`、`src-tauri/src/runtime/chat/context_builder.rs`、`src-tauri/src/runtime/tools/builtin/bash.rs`、`src/components/settings/panels/RuntimePanel.tsx`。

本 task 的包复用意图只覆盖**同一应用进程内**的新对话复用。关闭应用后再打开是否仍复用，是跨进程持久化维度，后续应单独写意图；不能混在同一条里。

---

## 意图-Runtime-001: 装包完成后，新对话不重复安装

**场景**
用户在一个对话里让 AI 使用本地 Runtime 安装并使用第三方 Python / Node 包。安装完成或确认已可用后，用户新建另一个对话再次使用同样的包，AI 应直接复用本地 Runtime 里的已安装包，只做可用性检查，不再执行安装命令。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 scope；记下 `{scope}`
3. 轮 A（允许补齐本地包）：
   1. 新建空对话：`tauri-pilot aijia new-task`
   2. 输入：
      ```
      请使用当前 AIjia Runtime 环境完成两个本地包检查：
      1. Python 包 humanize：如果不能 import，就按系统动态上下文给出的 uv pip install 模板安装；然后运行一段 Python，输出 humanize.intword(1234567) 的结果。
      2. Node 包 cowsay：如果 cowsay 命令不可用，就按系统动态上下文给出的 Runtime npm 模板安装 cowsay；安装后按系统动态上下文给出的 Node 命令目录绝对路径运行 cowsay "runtime-ok"。
      必须优先使用动态上下文里的 Runtime Python、Node、npm、uv 绝对路径，不要使用系统 PATH 里的裸 python / node / npm / pip，也不要用 npx 运行已安装的包。
      ```
   3. `tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 300`
   4. 记下 `conv_a=$(tauri-pilot aijia where --json | jq -r .activeConversationId)`
4. 轮 B（新对话复用本地包）：
   1. 新建空对话：`tauri-pilot aijia new-task`
   2. 输入：
      ```
      请再次使用当前 AIjia Runtime 环境检查并使用 Python 包 humanize 和 Node 包 cowsay。
      先检查它们是否已经可用；如果已经可用，不要安装任何 Python 包或 Node 包。
      然后运行 humanize.intword(7654321)，并从系统动态上下文里的 Node 命令目录用绝对路径运行 cowsay "runtime-reuse"。
      不要使用 npx 运行已安装的包。
      ```
   3. `tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 180`
   4. 记下 `conv_b=$(tauri-pilot aijia where --json | jq -r .activeConversationId)`
5. 从 `~/.renlijia/users/{scope}/conversations/{conv_a}/messages.jsonl` 和 `~/.renlijia/users/{scope}/conversations/{conv_b}/messages.jsonl` 提取 `role == "assistant"` 记录里的 `toolCalls[]`

**验收标准**

应该看到：
- 轮 A 的 `messages.jsonl` 中存在一条 `toolCalls[].name` 为 `Bash` 或 `PowerShell` 的记录，且 `toolCalls[].arguments.command` 包含 `humanize`
- 轮 A 的 `messages.jsonl` 中存在一条 `toolCalls[].name` 为 `Bash` 或 `PowerShell` 的记录，且 `toolCalls[].arguments.command` 包含 `cowsay`
- 轮 B 的 `messages.jsonl` 中存在一条 `toolCalls[].name` 为 `Bash` 或 `PowerShell` 的记录，且 `toolCalls[].arguments.command` 包含 `humanize`
- 轮 B 的 `messages.jsonl` 中存在一条 `toolCalls[].name` 为 `Bash` 或 `PowerShell` 的记录，且 `toolCalls[].arguments.command` 包含 `cowsay`
- 轮 B 的 `messages.jsonl` 中 Python 安装命令数量 `== 0`；Python 安装命令定义为 `toolCalls[].name` 为 `Bash` 或 `PowerShell`，且 `toolCalls[].arguments.command` 同时包含 `humanize` 和以下任一片段：`pip install`、`python -m pip install`
- 轮 B 的 `messages.jsonl` 中 Node 安装命令数量 `== 0`；Node 安装命令定义为 `toolCalls[].name` 为 `Bash` 或 `PowerShell`，且 `toolCalls[].arguments.command` 同时包含 `cowsay`，并命中以下任一条件：
  - 包含 `npx `
  - 包含 `pnpm install`、`pnpm add`、`yarn add`
  - 匹配 npm 安装：命令里出现 `npm` 可执行名（可以是裸 `npm`，也可以是 `"/abs/path/npm"`），且后面跟 `install` 或 `i`
- 轮 B 的最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 不为空
- 跑完轮 B 后 `tauri-pilot aijia health-check` 返回 ok

不应该看到：
- 轮 B 的 `toolCalls[].arguments.command` 同时包含 `humanize` 和 `install`
- 轮 B 的 `toolCalls[].arguments.command` 同时包含 `cowsay` 和 `install`
- 轮 B 的工具结果中包含 `ModuleNotFoundError`
- 轮 B 的工具结果中包含 `Cannot find module`
- 轮 B 的工具结果中包含 `command not found`
- 轮 B 的工具结果中包含 `npm error` 或 `npm ERR!`
- 任一轮对话 UI 中出现红色错误提示或「工具调用失败」类 toast

---

## 意图-Runtime-002: 指定位置检查后，新对话复用包

**场景**
用户让 AI 使用本地 Runtime 安装并使用一个普通 Python 包和一个普通 Node 包。新开对话再次使用同样的包时，AI 应先按动态上下文里的 Runtime 指定位置检查包是否已存在；如果已存在，只复用已安装包，不走系统全局环境检查，也不重复下载或安装。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 scope；记下 `{scope}`
3. 轮 A（允许补齐本地包）：
   1. 新建空对话：`tauri-pilot aijia new-task`
   2. 输入：
      ```
      请使用当前 AIjia Runtime 环境完成两个普通第三方包检查：
      1. Python 包 humanfriendly：先用动态上下文里的 Runtime Python 绝对路径执行 `import importlib.util; print(importlib.util.find_spec("humanfriendly") is not None)` 检查；如果输出不是 `True`，就按动态上下文给出的 uv pip install 模板安装到这个 Runtime Python；然后运行 humanfriendly.format_size(1536, binary=True)。
      2. Node 包 is-number：先按动态上下文里的 Node 全局包目录或 Runtime npm --prefix 模板检查 is-number 是否已安装；如果不能 require，就按动态上下文给出的 Runtime npm 模板安装到该 prefix；然后用动态上下文里的 Runtime Node 绝对路径和 Node 全局包目录运行 require("is-number")(42)。
      不要用系统 PATH 里的裸 python / py / node / npm / pip，不要用系统全局 npm 检查，不要用 npx。
      ```
   3. `tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 300`
   4. 记下 `conv_a=$(tauri-pilot aijia where --json | jq -r .activeConversationId)`
4. 轮 B（新对话复用本地包）：
   1. 新建空对话：`tauri-pilot aijia new-task`
   2. 输入：
      ```
      请再次使用当前 AIjia Runtime 环境检查并使用 Python 包 humanfriendly 和 Node 包 is-number。
      检查 humanfriendly 时，只能用动态上下文里的 Runtime Python 绝对路径执行 `import importlib.util; print(importlib.util.find_spec("humanfriendly") is not None)` 检查；如果输出为 `True`，不要安装 Python 包。
      检查 is-number 时，只能检查动态上下文里的 Node 全局包目录，或用动态上下文里的 Runtime npm 绝对路径并带 --prefix 指向 Runtime prefix；如果已经可用，不要安装 Node 包。
      然后运行 humanfriendly.format_size(2048, binary=True)，并用 Runtime Node + Node 全局包目录运行 require("is-number")("2048")。
      不要用系统 PATH 里的裸 python / py / node / npm / pip，不要用 npm list -g 但不带 --prefix 的全局检查，不要用 command -v / where / Get-Command 检查包，不要用 npx。
      ```
   3. `tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 180`
   4. 记下 `conv_b=$(tauri-pilot aijia where --json | jq -r .activeConversationId)`
5. 从 `~/.renlijia/users/{scope}/conversations/{conv_a}/messages.jsonl` 和 `~/.renlijia/users/{scope}/conversations/{conv_b}/messages.jsonl` 提取 `role == "assistant"` 记录里的 `toolCalls[]`
6. 从 `~/.renlijia/users/{scope}/conversations/{conv_b}/messages.jsonl` 提取 `role == "tool"` 记录里的工具输出文本

**验收标准**

应该看到：
- 轮 A 的 `messages.jsonl` 中存在一条 `toolCalls[].name` 为 `Bash` 或 `PowerShell` 的记录，且 `toolCalls[].arguments.command` 包含 `humanfriendly`
- 轮 A 的 `messages.jsonl` 中存在一条 `toolCalls[].name` 为 `Bash` 或 `PowerShell` 的记录，且 `toolCalls[].arguments.command` 包含 `is-number`
- 轮 B 的 `messages.jsonl` 中存在一条 `toolCalls[].name` 为 `Bash` 或 `PowerShell` 的记录，且 `toolCalls[].arguments.command` 包含 `humanfriendly`
- 轮 B 的 `messages.jsonl` 中存在一条 `toolCalls[].name` 为 `Bash` 或 `PowerShell` 的记录，且 `toolCalls[].arguments.command` 包含 `is-number`
- 轮 B 的 `messages.jsonl` 中存在一条 `toolCalls[].arguments.command` 同时包含 `humanfriendly` 和 `find_spec`
- 轮 B 的 `messages.jsonl` 中存在一条 `toolCalls[].arguments.command` 同时包含 `is-number` 和 `NODE_PATH`
- 轮 B 的 Python 安装命令数量 `== 0`；Python 安装命令定义为 `toolCalls[].arguments.command` 同时包含 `humanfriendly` 和 `install`
- 轮 B 的 Node 安装命令数量 `== 0`；Node 安装命令定义为 `toolCalls[].arguments.command` 同时包含 `is-number` 和 `install`
- 轮 B 的工具输出文本包含 `2 KiB`
- 轮 B 的工具输出文本包含 `true`
- 跑完轮 B 后 `tauri-pilot aijia health-check` 返回 ok

不应该看到：
- 轮 B 的 `toolCalls[].arguments.command` 包含 `npx`
- 轮 B 的 `toolCalls[].arguments.command` 包含 `command -v`
- 轮 B 的 `toolCalls[].arguments.command` 包含 `Get-Command`
- 轮 B 的 `toolCalls[].arguments.command` 包含 `where is-number`
- 轮 B 的 `toolCalls[].arguments.command` 包含 `npm list -g is-number` 且不包含 `--prefix`
- 轮 B 的 `toolCalls[].arguments.command` 包含 `pip show humanfriendly`
- 轮 B 的 `toolCalls[].arguments.command` 包含 `pip list`
- 轮 B 的工具输出文本包含 `ModuleNotFoundError`
- 轮 B 的工具输出文本包含 `Cannot find module`
- 轮 B 的工具输出文本包含 `command not found`
- 轮 B 的工具输出文本包含 `npm error` 或 `npm ERR!`
- 任一轮对话 UI 中出现红色错误提示或「工具调用失败」类 toast

---

## 意图-Runtime-003: 首次安装后，环境下载完成

**场景**
用户在一台从未运行过 AIjia 的新电脑或全新系统用户里安装 AIjia。首次启动后，应用应从默认运行期 manifest 下载 Runtime 压缩包，解压出 Node / Python / uv，并写入当前版本指针；不能把旧机器 cache 或 bundled fallback 当作下载成功。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 记录 `T0`；本意图只允许在刚完成干净安装并首次启动的测试机器或全新系统用户中执行
3. 推断 `{runtime_root}`：
   1. Windows：`%LOCALAPPDATA%\renlijia-runtimes\renlijia-primary-runtime`
   2. macOS：`~/Library/Caches/renlijia-runtimes/renlijia-primary-runtime`
   3. Linux：`~/.cache/renlijia-runtimes/renlijia-primary-runtime`
4. 从测试记录确认首次启动前 `{runtime_root}/current` 不存在；如果首次启动前已经存在，本意图记为 SKIPPED，换新电脑或全新系统用户重跑，不要删除当前用户的 runtime 目录硬造环境
5. 打开设置：`tauri-pilot aijia open-settings`
6. 等设置弹窗出现：`tauri-pilot aijia settings-wait`
7. 切到「Runtime」面板：`tauri-pilot aijia settings-select-panel --key runtime`
8. 等待「Runtime」面板不再显示加载态，并能看到 Node / Python / uv 诊断信息
9. 检查 `{runtime_root}/downloads/`
10. 读取 `{runtime_root}/current` 的文本内容，记为 `{current_pointer}`
11. 从 `{current_pointer}` 中去掉 `versions/` 前缀，记为 `{bundle_version}`
12. 检查 `{runtime_root}/{current_pointer}`
13. 检查 `~/.renlijia/logs/renlijia.log`

**验收标准**

应该看到：
- 文件 `{runtime_root}/current` 存在
- `{runtime_root}/current` 的文本内容不为空
- `{current_pointer}` 以 `versions/` 开头
- 目录 `{runtime_root}/{current_pointer}` 存在
- 目录 `{runtime_root}/downloads/` 存在
- `{runtime_root}/downloads/` 下存在文件名包含 `renlijia-primary-runtime` 的压缩包
- 该压缩包的 mtime 在 `T0 ± 5 分钟` 内
- 该压缩包的 size `> 0`
- `{runtime_root}/{current_pointer}/node/` 目录存在
- `{runtime_root}/{current_pointer}/python/` 目录存在
- `{runtime_root}/{current_pointer}/uv/` 目录存在
- 使用 `{runtime_root}/{current_pointer}` 下的 Node 可执行文件运行 `--version` 时，stdout 不为空
- 使用 `{runtime_root}/{current_pointer}` 下的 Python 可执行文件运行 `--version` 时，stdout 不为空
- 使用 `{runtime_root}/{current_pointer}` 下的 uv 可执行文件运行 `--version` 时，stdout 不为空
- `~/.renlijia/logs/renlijia.log` 中存在 `cache runtime initialized version={bundle_version}`

不应该看到：
- 测试记录中首次启动前 `{runtime_root}/current` 已存在
- `{runtime_root}/downloads/` 下没有 `renlijia-primary-runtime` 压缩包
- `~/.renlijia/logs/renlijia.log` 中出现 `manifest ensure failed; trying bundled fallback`
- `~/.renlijia/logs/renlijia.log` 中出现 `bundled fallback install start`
- Node / Python / uv 任一版本命令输出为空
- 任一检查步骤中出现 `checksum`、`extract`、`smoke test`、`network error`
