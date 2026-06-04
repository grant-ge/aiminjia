# Bug: 新对话重复安装 Runtime 第三方包

## 背景

客户反馈：每次对话都会重复安装 dws 相关的 Node/Python 包。用户侧看到的现象是“已经安装过一次，下一轮对话又安装”。

本次用 `pnpm dev:with-pilot` 做了同进程复现。没有关闭 app，第一轮安装 `humanize` 和 `cowsay`，第二轮新对话检查时两个包都显示 `missing`，随后又执行安装。

结论：这不是 dws skill 主动重复安装，也不是必须关闭应用才触发。根因在 Runtime 解析和后台 managed runtime reinstall。

## 复现摘要

启动命令：

```bash
rm -f /tmp/tauri-pilot-com.aijia.app.sock
PATH="$HOME/.cargo/bin:$PATH" pnpm dev:with-pilot
```

第一轮对话：

- Python: `humanize` 不能 import，执行 `uv pip install humanize --python "$PY"` 后成功。
- Node: `cowsay` 不存在，执行 `npm install -g cowsay --prefix "$NODE_DIR"` 后成功。

第二轮新对话：

```text
[python_check]
missing
[node_check]
missing
[node_runtime]
v22.15.0
[python_runtime]
Python 3.12.13
```

关键时间线：

```text
16:45:46 第一轮 Python 包安装成功
16:46:18 第一轮 Node 包安装成功
16:46:38 managed runtime 下载包被写入 downloads/
16:46:43 current 指针被更新
16:46:51 第二轮新对话开始
16:47:31 第二轮检查包为 missing
```

本地证据：

```text
/Users/a20250311/Library/Caches/renlijia-runtimes/renlijia-primary-runtime/downloads/renlijia-primary-runtime-2026.04.26-runtime.1-darwin-arm64.tar.gz
mtime = Jun 2 16:46:38 2026

/Users/a20250311/Library/Caches/renlijia-runtimes/renlijia-primary-runtime/current
mtime = Jun 2 16:46:43 2026
content = versions/2026.04.26-runtime.1
```

## 为什么代码需要做 Runtime 判断

这些判断本身是有必要的，不是多余逻辑。

app 在启动和执行工具时必须知道当前可用的 Runtime 在哪里，包括：

- Python 路径
- Node 路径
- npm/npx 路径
- uv/uvx 路径
- Node/Python 相关包目录

这些路径会被用于两个地方：

1. 注入给模型看的动态环境信息，让模型优先使用 AIjia Runtime 的绝对路径。
2. 旧实现还会注入给 BashTool 的 PATH，让 shell 里执行 npm/npx 时优先使用 AIjia Runtime。

如果不判断这些文件是否存在，app 可能把坏路径发给模型。旧 BashTool PATH 注入还会让工具执行环境和模型看到的绝对路径混在一起，导致职责不清。

所以代码需要判断：

- bundled runtime 是否完整可用；
- 如果 bundled 不可用，是否能 fallback 到 managed runtime；
- installed runtime 的 current 指针是否有效；
- runtime payload 是否具备最低可执行条件。

问题不在“要不要判断”，而在“判断用的目录和真实 Runtime 目录不一致”。

## 具体出错点

`RuntimeLayout` 现在认为 macOS/Linux 的包目录是：

```text
node/node_modules
python/lib/site-packages
```

代码位置：

- `src-tauri/src/runtime/dependencies/layout.rs`

```rust
pub fn node_modules(self) -> &'static str {
    "node/node_modules"
}

pub fn python_site_packages(self) -> &'static str {
    match self.platform {
        RuntimePlatform::WindowsX64 => "python/Lib/site-packages",
        RuntimePlatform::DarwinArm64
        | RuntimePlatform::DarwinX64
        | RuntimePlatform::LinuxX64 => "python/lib/site-packages",
    }
}
```

但 dev target 和真实 release `.app` 里的目录都是：

```text
runtime/darwin-arm64/node/lib/node_modules
runtime/darwin-arm64/python/lib/python3.12/site-packages
```

真实打包产物位置：

```text
src-tauri/target/release/bundle/macos/AIjia.app/Contents/Resources/runtime/darwin-arm64
```

实测真实 `.app` 里的 bundled 校验结果：

```text
OK   runtime/darwin-arm64/node/bin/node
OK   runtime/darwin-arm64/node/bin/npm
OK   runtime/darwin-arm64/node/bin/npx
OK   runtime/darwin-arm64/python/bin/python3
OK   runtime/darwin-arm64/uv/bin/uv
OK   runtime/darwin-arm64/uv/bin/uvx
FAIL runtime/darwin-arm64/node/node_modules
OK   runtime/darwin-arm64/node/lib/node_modules
FAIL runtime/darwin-arm64/python/lib/site-packages
OK   runtime/darwin-arm64/python/lib/python3.12/site-packages
```

也就是说，真实打出来的 `.app` 也会遇到这个 bundled false-negative：Runtime 可执行文件都在，但 bundled resolver 因为包目录路径检查失败，把 bundled runtime 判定为不可用。

## 谁使用了这些判断

### 1. bundled resolver 用 RuntimeLayout 判断 bundled runtime 是否可用

代码位置：

- `src-tauri/src/runtime/dependencies/bundled_resolver.rs`

调用链：

```text
BundledRuntimeResolver::workspace_dependencies()
  -> WorkspaceDependencies::from_install_dir_for_platform()
  -> RuntimeLayout::workspace_dependencies()
  -> validate_existing()
```

`validate_existing()` 不只检查 node/npm/python/uv 文件，也检查：

```rust
("node_modules", &deps.node_modules)
("python_site_packages", &deps.python_site_packages)
```

这一步导致 bundled resolver 因为错误目录失败。

### 2. app 启动时用 bundled resolver 决定是否跳过 OSS/managed ensure

代码位置：

- `src-tauri/src/lib.rs`

关键逻辑：

```rust
let bundled_ok = bundled_resolver.workspace_dependencies().is_ok();
if bundled_ok {
    log::info!("[runtime] bundled runtime ready; OSS ensure skipped on this launch");
} else {
    log::warn!("[runtime] bundled runtime unavailable; falling back to OSS ensure");
    tauri::async_runtime::spawn(async move {
        runtime_manager_bg.ensure_managed().await
    });
}
```

因为 bundled 被误判失败，app 启动后会异步执行 `ensure_managed()`。

### 3. managed ensure 会走 manifest 下载并 reinstall

代码位置：

- `src-tauri/src/runtime/dependencies/manager.rs`

关键逻辑：

```rust
self.installer.install_from_verified_archive(
    RuntimeInstallPlan::reinstall(fetched.bundle_version),
    &fetched.archive_path,
    &fetched.sha256,
)
```

当前默认 manifest 指向：

```text
2026.04.26-runtime.1
```

所以本次 fallback 下载了：

```text
renlijia-primary-runtime-2026.04.26-runtime.1-darwin-arm64.tar.gz
```

### 4. reinstall 会替换整个 version dir

代码位置：

- `src-tauri/src/runtime/dependencies/installer.rs`

关键逻辑：

```rust
let replaced_backup = self.replace_staging_with_version_dir(&staging_dir, &version_dir)?;
```

`replace_staging_with_version_dir()` 会：

```text
已有 version_dir -> staging/<version>.previous
新 staging_dir -> version_dir
```

这意味着 reinstall 是整个 Runtime 目录替换，不是增量更新。用户在第一轮装进 Runtime version dir 的第三方包会被一起清掉。

### 5. 对话和旧 BashTool PATH 注入都通过 resolver 读 Runtime 路径

代码位置：

- `src-tauri/src/transport/tauri_commands/chat.rs`
- `src-tauri/src/runtime/tools/builtin/shell_common.rs`
- `src-tauri/src/runtime/tools/builtin/bash.rs`
- `src-tauri/src/runtime/tools/builtin/powershell.rs`

用途：

- `get_env_info()` 调 `resolver.workspace_dependencies()`，把 Runtime 绝对路径写进动态上下文。
- 旧 `inject_bundled_runtime_path()` 调 `resolver.workspace_dependencies()`，把 Runtime Node bin 放进 BashTool / PowerShellTool 的 PATH。

本次日志里两轮对话都显示：

```text
[runtime] chain resolved via resolver[1]
```

这说明实际用的是 installed/managed runtime，而不是 resolver[0] 的 bundled runtime。

## 谁写的

以下来自 `git blame`，只说明代码引入者，不等同于责任归因。

| 代码 | blame |
| --- | --- |
| `RuntimeLayout::node_modules()` 返回 `node/node_modules` | `55287c37`, pzc, 2026-04-26, `feat: support multi-platform managed runtimes` |
| macOS/Linux `python_site_packages()` 返回 `python/lib/site-packages` | `492013ae`, pzc, 2026-04-27, `fix(storage): repair mixed transcript persistence` |
| `BundledRuntimeResolver` 和 bundled 校验 | `7a330b34`, grant, 2026-05-13, `feat: bundle Node/Python/uv runtime into installer (#1)` |
| app 启动时 bundled 不可用则 fallback 到 OSS ensure | 主要是 `7a330b34`, grant, 2026-05-13；RuntimeManager 初始化部分来自 `901239c2`, pzc, 2026-04-26 |
| manifest 下载后使用 `RuntimeInstallPlan::reinstall` | `901239c2`, pzc, 2026-04-26, `feat: add managed runtime manager` |
| installer 替换整个 version dir | `901239c2`, pzc, 2026-04-26, `feat: add managed runtime manager` |

## 为什么客户会看到 dws 重复安装

dws skill 文件本身没有写 npm/pip 安装逻辑。它只要求使用 `dws` 命令。

客户看到 dws 重复安装，是因为模型在新对话里发现 `dws` 或相关包不可用，于是再次安装。根因是 Runtime 包没有稳定持久化：

1. 第一轮把第三方包装进 Runtime version dir。
2. app 后台 managed runtime reinstall 替换 version dir。
3. 第三方包被清掉。
4. 新对话检查不到包。
5. 模型再次安装。

## 当前判断

本次 bug 的直接根因是：

1. bundled runtime 可用性校验用了不匹配真实布局的包目录，导致 bundled 被误判不可用。
2. app 启动 fallback 到 managed runtime 后，后台 ensure 走了 reinstall。
3. reinstall 替换了 Cache runtime version dir，第一轮刚安装的第三方包被删。

这里要区分两个概念：

- **Runtime 本体**：Node、npm、npx、Python、uv、uvx。
- **第三方包**：对话过程中安装的 dws、humanize、cowsay 等包。

本轮确认采用的产品边界是：第三方包可以继续放在 Cache runtime 的 version 目录里，接受 Runtime 版本升级后包需要重新安装一次；但同一个 Runtime 版本已经可用时，不能因为启动检查或新对话而反复 reinstall。

## bundled runtime 的作用

bundled runtime 是 app 包里自带的 Runtime payload。

它的作用应该是：

1. **离线兜底**：manifest/network 下载失败时，仍能初始化 Runtime。
2. **兜底安装来源**：Cache runtime 不存在且网络下载失败时，可以从 bundled runtime 复制/安装一份到 Cache。
3. **兜底修复来源**：Cache runtime 损坏且网络修复失败时，可以用 bundled runtime 修复。
4. **随包保底版本**：app 包里至少带一份可用 Runtime，避免完全依赖网络。

注意：bundled runtime 的版本不一定和线上 manifest 版本一致。本次本地实测就是：

```text
bundled runtime: 2026.05.13-runtime.1
manifest runtime: 2026.04.26-runtime.1
Cache runtime: 2026.04.26-runtime.1
```

所以启动逻辑不能假设三者版本一致，必须先读取版本，再按规则决策。

它不应该承担这些职责：

1. 不应该作为 BashTool 的长期执行目录。
2. 不应该作为 npm/uv 第三方包的安装目录。
3. 不应该在每次启动时覆盖 Cache runtime。
4. 不应该因为第三方包目录为空或布局差异，就判定整个 Runtime 不可用。

一句话：

```text
bundled runtime = app 内置的只读安装源
Cache runtime = 实际运行和安装第三方包的目录
```

## 完整修复方案

### 1. Cache runtime 作为唯一实际执行目录

后续动态上下文里给模型的路径应该来自 active Cache runtime：

```text
~/Library/Caches/renlijia-runtimes/renlijia-primary-runtime/versions/<version>/
```

也就是模型看到的 Node/npm/npx/Python/uv 路径应该来自 Cache runtime，而不是 app bundle 里的 bundled runtime。动态上下文不需要额外写 `Runtime 来源: cache` 或 `Runtime 版本: 2026.xx`，这些信息只用于日志和诊断。

BashTool 不应该自己决定使用 bundled 还是 Cache。BashTool 只执行命令。Runtime 选择应该由 RuntimeManager/RuntimeResolver 在 BashTool 之前完成。

推荐边界：

```text
RuntimeManager: 负责安装、修复、选择 active runtime
RuntimeResolver: 只返回 active Cache runtime 的路径
动态上下文: 只告诉模型使用 active Cache runtime 的绝对路径
BashTool: 只执行 shell 命令，不触发 install/reinstall
```

BashTool / PowerShellTool 不再注入 Runtime PATH。模型如果需要 Node/npm/Python/uv，应使用动态上下文里给出的 active Cache runtime 绝对路径。

### 2. 启动时先读取三方版本

启动决策前先读取三个版本：

```text
Cache version:
  ~/Library/Caches/renlijia-runtimes/renlijia-primary-runtime/current
  -> versions/<cache-version>/install.json.bundleVersion

Manifest version:
  runtime-manifest.json 中 runtimes.primary.version

Bundled version:
  app resource_dir/runtime/<platform>/bundled-version.json.bundleVersion
```

版本比较只做精确相等判断，不要按字符串大小猜“谁更新”。manifest、bundled、Cache 三者可能不一致，不一致本身不是错误。

### 3. 启动时先检查 Cache runtime

启动逻辑应该改成：

```text
1. 检查 Cache current runtime 是否存在且可用
2. 如果 Cache 可用：
   - active runtime = Cache
   - 不触发 managed ensure
   - 不触发 reinstall
   - 记录 Cache / Manifest / Bundled 版本是否一致
3. 如果 Cache 不可用：
   - 优先走 manifest/network 下载 manifest 指定版本的 managed runtime
   - 下载并安装到 Cache
   - active runtime = Cache
4. 如果 manifest/network 下载失败或不可用：
   - 再检查 bundled runtime
5. 如果 bundled 可用：
   - 从 bundled 安装/复制一份到 Cache
   - active runtime = Cache
6. 如果 Cache、manifest/network、bundled 都不可用：
   - Runtime 初始化失败，给出明确错误
```

这样可以保证：

```text
同一个 Runtime 版本已经可用时，不重复安装 Runtime 本体。
Cache 缺失时优先按 manifest 指定版本安装，bundled 只做离线/失败兜底。
```

### 4. bundled 校验只判断 Runtime 本体是否可用

bundled runtime 是否可用，核心应校验这些可执行文件：

```text
node/bin/node
node/bin/npm
node/bin/npx
python/bin/python3
uv/bin/uv
uv/bin/uvx
```

不应该因为下面这些目录不存在或路径不同，就判定 bundled runtime 不可用：

```text
node/node_modules
python/lib/site-packages
```

原因：

- Node 的真实全局包目录通常是 `node/lib/node_modules`。
- Python 的真实 site-packages 和 Python 版本有关，例如 `python/lib/python3.12/site-packages`。
- 这些目录属于包安装/解析细节，不应该决定 Runtime 本体是否可执行。

### 5. managed ensure 必须幂等

`ensure_managed()` 的语义应该是：

```text
如果当前 Cache runtime 可用，则直接返回 already-installed。
只有 current 缺失、损坏、或明确要求 reinstall 时，才替换目录。
```

manifest 下载后不能无条件走：

```rust
RuntimeInstallPlan::reinstall(fetched.bundle_version)
```

至少要先判断：

```text
当前 current 是否已经指向同一个 bundle version
当前 version dir 是否校验通过
```

如果同版本已经可用，不能 replace version dir。

### 6. Runtime 更新策略

需要明确区分“同版本启动”和“版本升级”。

同版本启动：

```text
Cache current 可用 -> 不复制、不下载、不 reinstall
```

app 升级后 bundled runtime 版本变化：

```text
正常在线场景：
  Cache 可用 -> 不因为 bundled 版本变化自动替换
  Cache 不可用 -> 优先从 manifest/network 下载 manifest 指定版本

离线/网络失败场景：
  如果 Cache 不可用，再用 app 当前 bundled runtime 初始化 Cache
  bundled 版本可能和 manifest 不一致，但要保证能用
```

如果 Cache 可用但版本和 manifest 不一致：

```text
默认行为：
  继续使用 Cache current
  不在普通启动/对话中偷换 Runtime
  只记录 version_mismatch 日志

显式升级行为：
  由 Runtime 设置页、发布策略或明确的后台升级任务触发
  安装到 Cache 的 versions/<manifest-version>/
  current 指向新版本
  第三方包可能需要重新安装一次
```

本次接受的边界是：

```text
显式 Runtime 版本升级后，第三方包重新安装一次可以接受。
同一 Runtime 版本内，新对话不能反复安装。
```

### 7. 日志补强

现有日志缺少关键事件，排查时只能靠文件 mtime 拼时间线。建议补这些日志：

```text
[runtime] cache probe start
[runtime] cache probe ok version=<version> path=<path>
[runtime] cache probe miss reason=<reason>
[runtime] manifest version version=<version>
[runtime] bundled version version=<version>
[runtime] version mismatch cache=<version> manifest=<version> bundled=<version>
[runtime] manifest install start version=<version>
[runtime] manifest install failed reason=<reason>
[runtime] bundled probe start path=<path> version=<version>
[runtime] bundled probe ok
[runtime] bundled probe miss reason=<reason>
[runtime] bootstrap cache from bundled version=<version>
[runtime] manifest ensure skipped reason=cache-current-valid
[runtime] reinstall start version=<version> reason=<reason>
[runtime] current pointer updated version=<version>
```

### 8. 测试覆盖

建议补以下测试：

1. `BundledRuntimeResolver` 测试：缺少 `node/node_modules` 或 `python/lib/site-packages` 时，只要核心可执行文件存在，不应判定 bundled runtime 不可用。
2. `RuntimeManager` 测试：Cache current 已存在且校验通过时，启动 ensure 不应下载、不应 reinstall。
3. `RuntimeManager` 测试：Cache、manifest、bundled 三者版本不一致但 Cache 可用时，继续使用 Cache，不 reinstall。
4. `RuntimeManager` 测试：Cache 缺失时，优先走 manifest/network 下载并安装到 Cache。
5. `RuntimeManager` 测试：Cache 缺失且 manifest/network 失败时，再从 bundled 初始化到 Cache，最后 resolver 返回 Cache 路径。
6. `BashTool` / `PowerShellTool` 测试：工具本身不注入 Runtime PATH；Runtime 路径只通过动态上下文提供。
7. 意图测试：同 app 进程新对话复用已安装包，不应再次执行 npm/uv install。

## 推荐改动文件

预计涉及：

- `src-tauri/src/runtime/dependencies/layout.rs`
  - 拆清楚 Runtime 本体路径和包目录路径，避免用错误包目录决定 Runtime 是否可用。
- `src-tauri/src/runtime/dependencies/bundled_resolver.rs`
  - bundled probe 只校验核心可执行文件。
- `src-tauri/src/runtime/dependencies/manager.rs`
  - 调整 ensure 语义，Cache 可用时不 reinstall。
  - Cache 不可用时先走 manifest/network，失败后再从 bundled 初始化 Cache。
- `src-tauri/src/runtime/dependencies/installer.rs`
  - 避免同版本可用时 replace version dir。
  - 必要时增加 copy/install-from-directory 能力。
- `src-tauri/src/lib.rs`
  - 启动逻辑改成 Cache first，再 manifest/network，再 bundled fallback。
- `src-tauri/src/transport/tauri_commands/chat.rs`
  - 动态上下文确认只展示 active Cache runtime 路径。
- `src-tauri/src/runtime/tools/builtin/shell_common.rs`
  - 删除 BashTool / PowerShellTool 的 Runtime PATH 注入逻辑。
- `src-tauri/tests/*`
  - 增加上述单元/集成测试。

## 验证命令

检查 bundled target 是否缺失当前 RuntimeLayout 要求的目录：

```bash
R=src-tauri/target/debug/runtime/darwin-arm64
test -d "$R/node/node_modules" || echo "missing node/node_modules"
test -d "$R/python/lib/site-packages" || echo "missing python/lib/site-packages"
test -d "$R/node/lib/node_modules" && echo "actual node lib node_modules exists"
test -d "$R/python/lib/python3.12/site-packages" && echo "actual python site-packages exists"
```

检查 managed runtime 是否在复现时间被下载/替换：

```bash
stat -f '%Sm %N' \
  "$HOME/Library/Caches/renlijia-runtimes/renlijia-primary-runtime/downloads/renlijia-primary-runtime-2026.04.26-runtime.1-darwin-arm64.tar.gz" \
  "$HOME/Library/Caches/renlijia-runtimes/renlijia-primary-runtime/current"
```

查看当前实际 resolver：

```bash
rg -n "chain resolved via resolver" "$HOME/.renlijia/logs/renlijia.log"
```

## 2026-06-02 打包版 E2E 结果

执行方式：

```bash
pnpm build:with-pilot
RUST_LOG=info ./src-tauri/target/release/bundle/macos/AIjia.app/Contents/MacOS/AIjia
tauri-pilot aijia health-check --json
```

说明：

- 使用的是 `src-tauri/target/release/bundle/macos/AIjia.app`，不是 `pnpm dev:with-pilot`。
- `pnpm build:with-pilot` 末尾因为缺少 `TAURI_SIGNING_PRIVATE_KEY` 返回 exit 1，但 `.app` 已生成；这和 `docs/e2e-release-build.md` 记录一致，不影响本地 E2E。
- 新包 mtime：`AIjia.app` 为 `Jun 2 18:45:53 2026`，二进制为 `Jun 2 18:45:52 2026`。
- 打包后的 bundled runtime 仍是实际发布布局：`node/lib/node_modules`、`python/lib/python3.12/site-packages` 存在；`node/node_modules`、`python/lib/site-packages` 不存在。

结果：

```text
packaged app health-check: ok=true
Cache current: versions/2026.04.26-runtime.1
Cache current mtime: Jun 2 16:46:43 2026
Cache install.json mtime: Jun 2 16:46:40 2026
```

结论：

- 修复后，打包版 app 启动没有替换 `~/Library/Caches/renlijia-runtimes/renlijia-primary-runtime/current`。
- Cache runtime 已可用时，启动不会因为 bundled runtime 的包目录布局差异触发 reinstall。
- 第一轮 Runtime 包检查安装了 `cowsay` 一次；第二轮新对话复用同一个 Cache runtime，`messages.jsonl` 中安装命令数量为 `0`。

第二轮有效命令：

```text
/.../python/bin/python3 -c "import humanize; print('humanize OK, version:', humanize.__version__)"
ls /.../node/bin/cowsay 2>/dev/null && echo "cowsay OK" || echo "cowsay NOT FOUND"
/.../python/bin/python3 -c "import humanize; print(humanize.intword(7654321))"
/.../node/bin/cowsay "runtime-reuse"
```

额外发现：

- 初次打包版 E2E 失败过一次，不是 Cache runtime 被重装，而是动态上下文没有告诉模型 Node 全局包应通过 `NODE_PATH=<runtime>/node/lib/node_modules` 检查，CLI 应通过 `<runtime>/node/bin/<命令>` 绝对路径运行。
- 已在 `src-tauri/src/runtime/chat/context_builder.rs` 补充 Node npm 安装模板、Node 全局包目录、Node 命令目录、`NODE_PATH` 检查模板，并明确不要用 `npx` 运行已安装包。

## 2026-06-02 Review 收口

后续 review 指出：只移除 bundled resolver 的包目录校验还不够，Cache 侧 `InstalledRuntimeResolver` 和 `RuntimeLayout` 仍然会把错误包目录当作 Runtime 可用性条件。

已收口：

- `RuntimeLayout` 的 macOS/Linux Node 包目录改为 `node/lib/node_modules`。
- `RuntimeLayout` 的 macOS/Linux Python site-packages 改为 `python/lib/python3.12/site-packages`。
- `InstalledRuntimeResolver` 不再用 Node/Python 第三方包目录判断 Cache runtime 是否可用；只校验核心可执行文件和 `install.json`。
- `RuntimeInstaller::validate_runtime_payload` 不再把第三方包目录缺失视作 Runtime payload 损坏。
- `RuntimeInstaller::install_from_local_archive` 在 archive 解压后也调用兼容目录创建，避免依赖 tar/zip 恰好带空目录。
- 已补测试覆盖：
  - Cache current 没有包目录时 resolver 仍可用。
  - archive 不带包目录时安装后会创建 Runtime 包目录。
  - 首次无 Cache、manifest 失败时，第一次 `workspace_dependencies()` 能从 bundled fallback 初始化 Cache，避免对话冷启动拿不到 Runtime 路径。

验证命令：

```bash
cd src-tauri
cargo test --test runtime_dependencies_resolver_test --test runtime_dependencies_installer_test --test runtime_dependencies_manager_test --test bundled_runtime_resolver_test
cargo test --test runtime_dependencies_manager_test
cargo check
```
