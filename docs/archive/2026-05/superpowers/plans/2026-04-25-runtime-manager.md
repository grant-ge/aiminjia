# Lotus 私有 Runtime Manager 实施计划

## 背景与问题

Lotus 的核心能力不是展示一个桌面 UI，而是在用户本机可靠地执行 Agent 需要的脚本、工具和 MCP server。只要 Agent 会分析文件、生成图表、处理表格、调用 MCP 或运行自动化脚本，就一定会碰到 Python、Node、uv、npm 这些运行环境。

如果这些环境依赖用户自己安装，产品能力就会变成“不确定”：有些机器能跑，有些机器不能跑；有些机器版本正确，有些机器因为 PATH、Python 版本、Node 版本或包管理器差异直接失败。

当前 Lotus 已有一个旧实现：`scripts/setup-python.sh` 会下载 `python-build-standalone` 并安装到 `src-tauri/python-runtime`，`tauri.conf.json` 还会把 `src/python` 打包成 `python-runtime` 资源。这个方案在早期能快速让 Python 工具跑起来，但它不适合作为长期架构：它把运行时和 App 包、源码目录、CI 缓存、Tauri resource 绑在一起，也无法统一管理 Node、uv、MCP server 和后续插件运行时。

因此，这个计划的核心不是“把 Python 换个目录下载”，而是把“本机是否能执行代码”从用户环境问题，变成 Lotus 自己可控制、可验证、可恢复的基础设施问题。

---

## 核心判断

### 为什么要做 Runtime Manager

Runtime Manager 要回答几个基础问题：

- 这次工具执行到底用了哪个 Python / Node？
- 这个 runtime 是不是 Lotus 安装并校验过的？
- 用户机器没有装 Python / Node 时是否还能工作？
- runtime 损坏、升级失败、版本不兼容时是否能恢复？
- App 升级和 runtime 升级是否可以解耦？
- MCP、Python 工具、Node 工具是否走同一套能力边界？

如果这些问题不先解决，后面继续加 execute_python、MCP、文档处理、表格分析、插件系统，都会把“不确定的本机环境”扩散到更多模块里。

### 为什么不能继续用 `src-tauri/python-runtime`

`src-tauri/python-runtime` 属于旧版本内容，只能作为迁移期 fallback，不能继续作为目标架构。

主要原因：

1. **运行时和 App 包强耦合。** Python 升级、依赖升级、修复 runtime 损坏，都变成 App 发版问题。
2. **目录语义不对。** `src-tauri` 是源码/构建目录，不应该成为生产 runtime 的长期来源。
3. **能力覆盖不完整。** 旧方案只能解释 Python，不能统一 Node、npm/npx、uv/uvx、MCP server 和插件运行时。
4. **会鼓励业务代码继续拼路径。** 一旦工具层知道 `python-runtime/bin/python3`，runtime 能力就会散落在各处，后面很难统一权限、升级、健康检查和回滚。

因此新方案要把它从 Tauri resources、CI cache、setup 脚本和生产执行链路里移除。

### 为什么选择 `~/.cache/renlijia-runtimes`

runtime 本质上是可重建缓存，不是用户数据。用户的 workspace、sessions、skills、settings 应该留在现有用户数据目录；Node/Python/uv 这种可重新下载、可校验、可清理的内容应该放到 cache 下。

目标目录是：

```text
~/.cache/renlijia-runtimes/
  renlijia-primary-runtime/
    current
    versions/
      2026.04.25/
        install.json
        dependencies/
          node/
          python/
          uv
          uvx
    downloads/
    staging/
```

这个结构的价值在于：

- `versions/` 支持多版本并存。
- `staging/` 支持先下载解压，再原子切换。
- `current` 支持回滚和健康检查。
- `downloads/` 支持断点、缓存和重复安装复用。
- 删除整个 `renlijia-runtimes` 不会删除用户数据，只会触发 runtime 重建。

### 什么时候触发下载

Runtime 下载不应该发生在系统安装器阶段，也不应该在每次启动时阻塞主界面。推荐采用“启动后预检查 + 首次使用强保证”的组合策略：

1. **App 启动后后台预检查。** 启动时创建 `RuntimeResolver`，后台检查 `~/.cache/renlijia-runtimes/renlijia-primary-runtime/current` 是否存在、版本是否匹配、health 是否通过。这个阶段可以预热，但不应该阻塞聊天 UI。
2. **首次本地执行前强制 ensure。** 当用户第一次触发 `execute_python`、chart/report、file_load、MCP stdio、Node 脚本、文档/表格处理等需要本地 runtime 的能力时，工具执行链路必须调用 `RuntimeResolver.workspace_dependencies()`，由它触发 `RuntimeInstaller.ensure(...)`。如果 runtime 缺失，就在这里下载、校验、解压和 smoke test。
3. **用户主动修复时重新下载。** 设置页提供“检查运行环境 / 重新安装运行环境 / 清理旧版本”，这些操作调用 `runtime_ensure` 或 `runtime_reinstall`。
4. **升级 manifest 时懒更新。** App 可以拿到新的 runtime manifest，但不要立刻破坏当前可用版本。只有当新版本下载、校验、smoke test 全部通过后，才切换 `current`。

因此下载触发点是：

```text
App 启动
  -> 后台 health check，尽量不阻塞 UI

首次需要本地执行
  -> RuntimeResolver.workspace_dependencies()
  -> RuntimeInstaller.ensure()
  -> 缺失才下载

设置页用户点击修复
  -> runtime_reinstall
  -> 强制重新下载/解压/切换
```

这样可以避免安装包巨大，也避免用户只是打开 App 聊天时被迫等待几百 MB runtime 下载。

### 从哪里下载

下载源应该分三层，避免把某一个 URL 写死在业务代码里：

1. **官方 Renlijia runtime manifest。** 生产默认从 Renlijia 控制的 manifest 获取下载地址、版本和 sha256。manifest 可以托管在 OSS/CDN，例如 `https://download.renlijia.com/runtimes/manifest.json`。
2. **上游官方发行源。** runtime 包本身可以来自官方 Node.js、python-build-standalone、uv GitHub release，也可以由 Renlijia CI 预下载、校验后重新打包上传到自有 CDN。生产更推荐后者，因为下载速度、可用性和 hash pinning 都更可控。
3. **企业/离线镜像。** 支持环境变量或设置项覆盖 manifest base URL，例如 `RENLIJIA_RUNTIME_MIRROR`。企业内网可以把同样的 tarball/zip 和 manifest 放到私有镜像。

推荐生产路径是 Renlijia 自己维护 runtime bundle：

```text
Renlijia CI
  -> 下载 Node 官方包 / python-build-standalone / uv
  -> 重新组织目录为 renlijia-primary-runtime dependencies
  -> 生成 tar.gz/zip
  -> 计算 sha256
  -> 生成 manifest.json
  -> 上传 OSS/CDN

用户 App
  -> 下载 manifest.json
  -> 下载对应平台 artifact
  -> sha256 校验
  -> 解压到 staging
  -> smoke test
  -> 切换 current
```

完整方案要求接入真实生产 manifest 和 runtime artifact 下载源。开发环境可以额外保留本地测试 manifest，但生产路径必须由签名/校验过的 manifest 驱动，不能依赖本地 fixture。

生产 manifest 至少包含：

```json
{
  "bundleVersion": "2026.04.25",
  "channel": "stable",
  "minimumAppVersion": "0.4.16",
  "defaultProvider": "renlijia-bundle",
  "runtimes": {
    "node": {
      "version": "22.19.0",
      "platforms": {
        "darwin-arm64": {
          "url": "https://download.renlijia.com/runtimes/2026.04.25/node-darwin-arm64.tar.gz",
          "sha256": "...",
          "sizeBytes": 123456789
        }
      }
    },
    "python": {
      "version": "3.12.13",
      "platforms": {}
    },
    "uv": {
      "version": "0.7.13",
      "platforms": {}
    }
  }
}
```

App 不信任 URL 本身，只信任 manifest 中 pin 住的 sha256。下载、解压、smoke test 任一步失败，都不能切换 `current`。

### 包管理结论：Renlijia 不做通用包管理

结论很明确：Renlijia 不做 npm/PyPI 这种通用包管理，也不自己解析第三方依赖图。包管理不是 Renlijia 的专业领域，强行自研会变成维护成本很高的“半个 npm + 半个 pip/uv”。

Renlijia 只负责一件事：**交付和管理产品需要的基础 runtime bundle**。

```text
Renlijia Runtime Manager 负责
  -> 下载 runtime bundle
  -> 校验 sha256
  -> 解压 staging
  -> smoke test
  -> 切换 current
  -> 回滚
  -> 暴露 node/python/uv 的绝对路径

第三方生态工具负责
  -> Node 包安装和执行
  -> Python 包安装和虚拟环境
  -> workspace 依赖解析
  -> lockfile / package metadata 处理
```

默认只内置两套生态工具，不引入额外总包管理器：

```text
Node 侧
  node
  npm
  npx
  corepack

Python 侧
  python3
  pip
  uv
  uvx
```

workspace 依赖处理规则：

```text
Node workspace
  package.json / package-lock.json / pnpm-lock.yaml / yarn.lock
  -> npm / npx / corepack 处理

Python workspace
  pyproject.toml / uv.lock / requirements.txt
  -> uv / uvx / pip 处理
```

Renlijia 不负责：

- npm 依赖解析
- PyPI 依赖解析
- lockfile 语义实现
- 原生扩展编译策略
- 包撤回和安全公告处理
- 多语言通用包管理

Renlijia 负责把这些工具稳定地带到用户机器上，并在执行前用权限系统控制它们何时能联网、写 workspace、安装依赖或运行脚本。

为什么不默认引入 Pixi/mise/asdf/aqua 这类总包管理器？

- 只需要 Node 和 Python，额外总包管理器会让系统更复杂。
- 用户侧问题会从“下载 Node/Python”变成“先安装并调试另一个包管理器”。
- 这些工具有自己的 shims、cache、PATH、配置语义，容易和“不污染系统环境”冲突。
- Renlijia 仍然要自己做 UI 进度、错误恢复、sha256、staging、回滚和企业镜像。

因此完整方案采用最小稳定边界：

```text
Renlijia 管 runtime bundle。
Node 包交给 npm/corepack/npx。
Python 包交给 uv/pip。
```

### runtime artifact 里面有什么

`runtime artifact` 是 Renlijia 发布或引用的运行环境制品，不是 npm 包，也不是 Python 包。它的目标是把 Agent 本地执行所需的基础二进制工具放到一个可校验、可解压、可切换的单位里。

长期生产推荐一个平台一个 Renlijia bundle：

```text
renlijia-primary-runtime-darwin-arm64.tar.gz
renlijia-primary-runtime-darwin-x64.tar.gz
renlijia-primary-runtime-win32-x64.zip
renlijia-primary-runtime-linux-x64.tar.gz
```

包内结构：

```text
install.json
dependencies/
  node/
    bin/node
    bin/npm
    bin/npx
    bin/corepack
    node_modules/
  python/
    bin/python3
    bin/pip
    bin/pip3
    lib/python3.12/
  uv
  uvx
```

`install.json` 记录 bundle 内相对路径和版本：

```json
{
  "bundleVersion": "2026.04.25",
  "platform": "darwin-arm64",
  "node": {
    "version": "22.19.0",
    "path": "dependencies/node/bin/node"
  },
  "python": {
    "version": "3.12.13",
    "path": "dependencies/python/bin/python3"
  },
  "uv": {
    "version": "0.7.13",
    "path": "dependencies/uv"
  }
}
```

解压后落在：

```text
~/.cache/renlijia-runtimes/
  renlijia-primary-runtime/
    versions/
      2026.04.25/
        install.json
        dependencies/
          node/
          python/
          uv
          uvx
    current
```

### 可以先用公网源，但不能硬编码公网 URL

完整方案支持两种 artifact provider：

```text
OfficialSourceProvider
  -> manifest 指向官方公网包
  -> Node 来自 nodejs.org
  -> Python 来自 python-build-standalone release
  -> uv 来自 uv release
  -> App 下载多个上游包并整理成统一目录

RenlijiaBundleProvider
  -> manifest 指向 Renlijia 自己的 runtime bundle
  -> App 下载一个平台包
  -> 直接解压、校验、smoke test、切换 current
```

过渡期可以使用公网源代替自有 bundle，但必须满足一个原则：

```text
App -> Renlijia manifest -> 官方公网 URL
```

不能写成：

```text
App -> 代码里硬编码 nodejs.org / GitHub release URL
```

这样做的原因是后续从公网源切换到 Renlijia CDN 时，只需要改 manifest，不需要改 App 代码。

推荐落地顺序：

1. 先实现 manifest 驱动的 `OfficialSourceProvider`，用于验证下载、校验、解压、health、current 切换全链路。
2. 同时保留 `RenlijiaBundleProvider` 的 schema 和目录语义。
3. 生产稳定后切换 manifest，让默认 provider 指向 Renlijia 自己预打包的 primary runtime artifact。

长期生产默认必须是 `RenlijiaBundleProvider`，因为它目录结构统一、下载次数少、可预置内置依赖，也更适合企业镜像和离线包。

### 下载后如何安装

下载只是第一步。Runtime artifact 下载完成后，必须由 `RuntimeInstaller` 安装到 Renlijia 管理目录。这里的“安装”不是系统安装，不写 `/usr/local/bin`，不修改用户 shell profile，不污染系统 PATH。所有文件只写入：

```text
~/.cache/renlijia-runtimes/
  renlijia-primary-runtime/
    downloads/
    staging/
    versions/
    current
```

安装流程分 provider 处理。

**RenlijiaBundleProvider：** 下载到的是 Renlijia 已经预打包好的标准 runtime bundle。

```text
下载 renlijia-primary-runtime-<platform>.tar.gz / .zip
  -> 保存到 downloads/
  -> 校验 sha256
  -> 解压到 staging/<install-id>/
  -> 检查 install.json
  -> smoke test
       node -v
       python3 --version
       uv --version
  -> promote 到 versions/<bundleVersion>/
  -> current 指向 versions/<bundleVersion>
  -> 返回 WorkspaceDependencies
```

**OfficialSourceProvider：** 下载到的是 Node/Python/uv 各自的官方包，结构不统一，所以多一步归一化。

```text
下载 Node 官方包、python-build-standalone、uv release
  -> 分别保存到 downloads/
  -> 分别校验 sha256
  -> 解压到 staging/<install-id>/sources/
  -> 归一化为 Renlijia 标准结构
       dependencies/node/
       dependencies/python/
       dependencies/uv
       dependencies/uvx
  -> 生成 install.json
  -> smoke test
       node -v
       npm -v
       python3 --version
       uv --version
  -> promote 到 versions/<bundleVersion>/
  -> current 指向 versions/<bundleVersion>
  -> 返回 WorkspaceDependencies
```

失败处理必须保持原子性：

- 下载失败：保留旧 `current`，清理本次 staging。
- sha256 失败：删除损坏 archive，不切换版本。
- 解压失败：清理 staging，不切换版本。
- smoke test 失败：保留 staging 日志，不切换版本。
- promote 失败：保留旧版本，不破坏当前可用 runtime。

因此，业务工具看到的永远是已经安装好的 `current`，不会拿到半下载、半解压或未通过 smoke test 的 runtime。

### 完整下载流程：脚本生产包，App 下载包

完整方案分成两个运行环境，职责不能混在一起：

```text
发布侧 CI / 构建机
  -> 运行 scripts/build-runtime-bundle.py
  -> 生成 runtime artifact 和 manifest
  -> 上传 OSS/CDN

用户侧 Lotus App
  -> Rust RuntimeInstaller 读取 manifest
  -> 下载对应平台 artifact
  -> 校验、解压、smoke test、切换 current
```

也就是说，需要提供下载/打包脚本，但这个脚本不是给用户 App 调用的。用户机器上不运行 `setup-python.sh`、`setup-node.sh` 这类 shell 脚本；用户侧下载必须由 App 内 Rust 代码完成。

发布侧脚本职责：

```text
scripts/build-runtime-bundle.py
  -> 下载 Node 官方包 / python-build-standalone / uv
  -> 校验上游包 hash 或 release checksum
  -> 整理成 renlijia-primary-runtime 统一目录结构
  -> 安装/预置内置 Node packages 和 Python packages
  -> 删除无用缓存和临时文件
  -> 打包为 tar.gz / zip
  -> 计算 artifact sha256 和 sizeBytes
  -> 生成 manifest.json
  -> 输出 dist/runtimes/<bundleVersion>/
```

发布产物示例：

```text
dist/runtimes/2026.04.25/
  manifest.json
  renlijia-primary-runtime-darwin-arm64.tar.gz
  renlijia-primary-runtime-darwin-x64.tar.gz
  renlijia-primary-runtime-win32-x64.zip
  renlijia-primary-runtime-linux-x64.tar.gz
```

runtime 包内部结构：

```text
install.json
dependencies/
  node/
    bin/node
    bin/npm
    bin/npx
    node_modules/
  python/
    bin/python3
    lib/python3.12/
  uv
  uvx
```

用户侧 App 代码职责：

```text
RuntimeResolver.workspace_dependencies()
  -> RuntimeInstaller.ensure()
  -> ManifestClient.fetch()
  -> RuntimeManifest.select(platform)
  -> RuntimeDownloader.download(url, downloads/)
  -> verify_sha256(archive, sha256)
  -> ArchiveExtractor.extract(archive, staging/)
  -> RuntimeHealthChecker.smoke_test(staging/)
  -> RuntimeInstaller.promote(staging, versions/<bundleVersion>)
  -> current 指向新版本
  -> 返回 WorkspaceDependencies 绝对路径
```

用户侧代码不能这样做：

```text
Command::new("bash").arg("scripts/setup-python.sh")
```

原因是 App 需要结构化错误、下载进度、取消、重试、代理、hash 校验、解压安全、失败回滚和前端状态展示。shell 脚本不适合作为用户侧 runtime installer。

旧的 `scripts/setup-python.sh` 是开发期遗留脚本，目标不是继续扩展它。完整方案需要新增发布侧 runtime 打包脚本，用于生成正式 artifact 和 manifest；旧脚本最终移除。

### 为什么要有 `RuntimeResolver`

最重要的架构边界是：业务工具不能自己知道 runtime 放在哪里。

如果 `execute_python`、chart、report、file_load、MCP 各自拼路径，就会出现多个事实来源：有的走系统 Python，有的走 `src-tauri/python-runtime`，有的走新 cache runtime。这样不仅难调试，也会破坏权限、审计和升级。

因此所有工具只能通过 `RuntimeResolver` 获取：

```text
WorkspaceDependencies
  node
  npm
  npx
  python
  uv
  uvx
  node_modules
  python_site_packages
```

这让 runtime 决策集中到一个地方：

```text
工具需要执行
  -> CapabilityContext / MCP connection 获取 RuntimeResolver
  -> RuntimeResolver 返回绝对路径
  -> Command::new(绝对路径)
```

这样做的收益是：

- 工具层不用关心平台差异。
- 后续 runtime 目录迁移不影响工具代码。
- 权限、健康检查、安装、升级、回滚可以统一处理。
- 可以明确禁止系统 PATH 偷偷参与关键工具执行。

### 为什么 CapabilityContext 里要可选注入

不是所有工具都需要 runtime。如果把 `RuntimeResolver` 做成 `CapabilityContext` 的必填字段，会破坏大量现有测试和不需要本地执行的工具上下文。正确做法是可选注入：

```text
CapabilityContext
  runtime_resolver: Option<Arc<dyn RuntimeResolver>>
```

需要本地执行的工具调用 `workspace_dependencies()`；不需要 runtime 的工具不受影响。这样迁移可以逐步推进，不会因为一个字段导致全仓库构造器同时爆炸。

### 为什么 MCP 要在 connection spawn 边界解析

MCP stdio server 的真正执行点在 `StdioMcpConnection::connect()`。如果在 manager 层解析 `${renlijia.node}`，看起来配置被处理了，但真实 spawn 仍可能绕过解析逻辑。占位符必须在最靠近 `Command::new(...)` 的地方处理，才能保证最终执行的一定是 resolver 返回的绝对路径。

目标链路是：

```text
McpServerConfig.endpoint = "${renlijia.node} server.js"
  -> StdioMcpConnection::connect()
  -> parse command
  -> resolve_mcp_command_template()
  -> Command::new(deps.node)
```

### 为什么前端 API 必须先有后端 command

runtime health 是真实系统状态，不是前端展示状态。前端可以有 `getRuntimeHealth()`、`ensureRuntime()`、`reinstallRuntime()`，但这些 wrapper 必须对应后端 Tauri command。否则前端测试 mock 能过，真实 App 一调用就失败。

所以顺序必须是：

```text
后端 RuntimeResolver / health payload / Tauri command
  -> 前端 tauri.ts wrapper
  -> runtimeStore
  -> settings UI
```

不能反过来先写 UI mock。

---

## 目标状态与边界

**目标：** 为 Lotus 增加 Codex/Real 风格的私有运行时管理层，让 Node、Python、uv 由应用托管、下载、校验、解析和执行，而不是依赖用户系统环境。

**架构：** 新增 `runtime/dependencies` 作为唯一 Runtime 解析与安装边界，工具执行层只通过 `RuntimeResolver` 获取绝对路径。完整方案直接以 `~/.cache/renlijia-runtimes/renlijia-primary-runtime` 为目标运行时目录，`src-tauri/python-runtime` 只作为迁移期 legacy fallback，完成注入链路迁移后从打包资源、CI 缓存和 setup 脚本中移除。

**技术栈：** Tauri 2.x、Rust、React/TypeScript、Vitest、Cargo tests、python-build-standalone、Node.js 官方发行包、uv/uvx、现有 `RuntimeTool` / `CapabilityContext` / MCP Runtime。

目标状态：

```text
Lotus App 本体
  不污染系统 PATH
  不强制用户预装 node/python

RuntimeManager
  管理 ~/.cache/renlijia-runtimes/renlijia-primary-runtime/current
  提供 node/python/uv/uvx/npm/npx 绝对路径
  支持 manifest、下载、sha256、解压、smoke test、原子切换、回滚
  使用 app_lib crate 名称编写 Rust 集成测试

Tool Execution
  execute_python / bash / MCP / 文档工具统一通过 RuntimeResolver 拿路径
  覆盖现有 Python 注入链路，包含 registry、file_load、worker_runtime 和 builtin Python/Chart/Report 工具
  MCP stdio 占位符解析发生在 connection spawn 边界
  用户依赖与 Lotus 内置依赖隔离
  保留现有权限、取消、事件、workspace sandbox 约束
```

非目标：

- 不把 Node/Python 安装进 `/usr/local/bin`、系统 Python、系统 npm。
- 不在第一阶段实现完整 Docker/VM 沙箱。
- 不在第一阶段重写所有工具系统；只把 runtime 路径能力收口到明确接口。
- 不把所有 Python/Node 依赖塞进一个不可维护的大环境；内置工具依赖和 workspace 依赖要分层。

最终实现约束：

- 用户可见 runtime 根目录和 bundle id 统一使用 `renlijia`：`~/.cache/renlijia-runtimes/renlijia-primary-runtime`。
- `CapabilityContext` 使用可选 `RuntimeResolver` 和 builder 注入，不破坏不需要 runtime 的工具上下文。
- 前端 runtime API 必须有对应后端 Tauri command，不允许只写 mock wrapper。
- manifest 必须覆盖生产下载源、平台 artifact、sha256、版本、回滚信息和企业镜像覆盖；测试 fixture 只能用于测试。
- Installer 从一开始锁定 `versions/<version>` + `current` pointer 语义。
- `src-tauri/python-runtime` 是 legacy 路径，只允许在迁移期作为 fallback；计划完成后必须移除打包资源配置和构建脚本依赖。

---

## 迁移策略

迁移必须分阶段，不能直接删除旧 runtime。原因是当前还有多处生产路径在调用 `resolve_python_path()` 或传递 `python_binary/python_home`。

正确顺序是：

1. **建立新边界。** 新增 `runtime/dependencies`，完成平台识别、路径计算、manifest、health、checksum、installer 骨架。
2. **让 PythonRunner 支持 resolver。** 新增 resolver 构造入口，但暂时保留 `resolve_python_path()` fallback。
3. **迁移所有 Python 注入点。** 覆盖 `plugin/registry.rs`、`file_load.rs`、`worker_runtime.rs`、builtin Python/Chart/Report 工具。
4. **接入 MCP 和前端健康状态。** MCP stdio 解析 `${renlijia.*}`，前端通过真实 Tauri command 读取状态。
5. **删除旧 `python-runtime`。** 当生产链路不再依赖旧路径后，移除 `tauri.conf.json` resource、setup-python 脚本输出、CI cache/bootstrap 和 symlink 依赖。

完成后，Lotus 应该具备这些性质：

- 用户不需要预装 Python、Node、uv。
- App 不污染系统 PATH。
- 工具执行使用绝对路径，可审计、可复现。
- runtime 可以独立于 App 升级。
- `src-tauri/python-runtime` 不再进入生产包。
- 后续 browser runtime、office runtime、plugin runtime 可以复用同一套机制。

---


## 当前执行状态校准（2026-04-26）

本计划曾出现“文档已勾选但实现未完全落地”的问题。后续执行以本节为准：只有下列已有测试覆盖的项可以视为已完成；未列入完成项的内容，即使正文旧段落存在 `[x]`，也必须继续实现或重新拆任务。

### 已完成并已有验证的关键项

- [x] Runtime 根目录和 bundle 命名统一为 `renlijia-runtimes` / `renlijia-primary-runtime`。
- [x] `RuntimePaths`、manifest schema、checksum、zip slip 防护、`versions/<version>` + `current` pointer 已实现。
- [x] artifact 安装链路已执行 sha256 校验、staging 解压、payload 校验、staging smoke test，再切换 `current`。
- [x] `RuntimeManager` 支持 manifest source：file fixture、HTTPS manifest、HTTPS artifact 下载到 `downloads/` 后安装。
- [x] `runtime_ensure` / `runtime_reinstall` 后端 command 不再直接把 dev stub 当生产安装成功；有 manifest 配置时走 managed manifest 链路。
- [x] 注入给业务工具和 MCP 的 resolver 可在首次 `workspace_dependencies()` 时触发 manifest ensure，避免工具静默走系统 Python/Node。
- [x] MCP stdio 支持 `${renlijia.python}`、`${renlijia.node}`、`${renlijia.npm}`、`${renlijia.npx}`、`${renlijia.uv}`、`${renlijia.uvx}`。

### 仍未完成，不能标成完成

- [x] 下载进度事件、取消下载、重试/backoff、临时 `.part` 文件恢复入口。
- [x] 后端/前端 API 支持清理旧 runtime 版本和保留数量。
- [x] manifest 生产字段：channel、minimumAppVersion、sizeBytes、rollback、mirrors/defaultProvider。
- [x] tar.gz artifact 支持。
- [x] `install.json` 基础完整元数据：平台、runtime 分组、相对路径；安装时间/artifact digest 可后续从 manifest 注入。
- [x] 发布侧 artifact 构建脚本已提交；真实公网 manifest 内容仍由发布流水线生成和托管。

### 下载触发规则

- 启动时默认使用内置 OSS manifest 后台 ensure，不阻塞主界面；`RENLIJIA_RUNTIME_MANIFEST_URL` 只用于覆盖默认源。
- 设置页调用 `runtime_ensure` / `runtime_reinstall` 时触发 manifest 下载与安装。
- 本地工具首次执行前调用 `RuntimeResolver::workspace_dependencies()`，如果 `current` 缺失或损坏，会基于内置或覆盖 manifest 自动 ensure；失败时返回明确错误，不回退系统 runtime。

## 验证策略

Runtime Manager 的验证目标不是“代码能编译”，而是证明它不会污染系统、不会在失败时破坏旧 runtime、业务工具确实走托管 runtime，并且真实下载链路可用。验证分层如下。

### 1. 普通测试不真实下载

日常单测和集成测试不能下载真实 Node/Python/uv。普通测试使用 fake 组件：

```text
FakeManifestClient
FakeRuntimeDownloader
FakeArchiveExtractor
FakeRuntimeHealthChecker
```

这些 fake 用来验证状态机顺序：

```text
fetch manifest
  -> select artifact
  -> download archive
  -> verify sha256
  -> extract staging
  -> smoke test
  -> promote versions
  -> switch current
```

### 2. 必须证明写入范围受限

Installer 只允许写：

```text
~/.cache/renlijia-runtimes/renlijia-primary-runtime/downloads
~/.cache/renlijia-runtimes/renlijia-primary-runtime/staging
~/.cache/renlijia-runtimes/renlijia-primary-runtime/versions
~/.cache/renlijia-runtimes/renlijia-primary-runtime/current
```

必须测试禁止写：

```text
/usr/local/bin
/opt/homebrew/bin
/etc/profile
~/.zshrc
~/.bashrc
```

### 3. 必须证明失败不切换 current

以下失败都必须保留旧版本：

```text
manifest 拉取失败
download 失败
sha256 不匹配
archive 解压失败
目录归一化失败
smoke test 失败
promote 失败
```

关键断言是：

```text
失败前 current = versions/2026.04.01
失败后 current 仍然 = versions/2026.04.01
失败版本不会出现在 versions/2026.04.25
```

### 4. 必须证明成功路径完整

成功安装后必须同时满足：

```text
archive 存在 downloads/
staging 被使用并清理或归档
versions/<bundleVersion>/install.json 存在
current 指向 versions/<bundleVersion>
WorkspaceDependencies 返回绝对路径
node/python/uv 路径都位于 versions/<bundleVersion>/dependencies 下
```

### 5. 必须证明业务工具真的走 resolver

不能只测试 `RuntimeResolver` 自己返回路径，还要测试业务入口：

```text
PythonRunner 使用 deps.python
ExecutePythonRuntimeTool 使用 deps.python
chart/report/file_load 使用托管 python
MCP stdio `${renlijia.node}` 解析到 deps.node
Tauri runtime_get_health 返回 resolver 的状态
```

### 6. 必须证明旧 `python-runtime` 已移除

完成迁移后运行静态搜索：

```bash
rg -n "src-tauri/python-runtime|src/python.:.python-runtime|python-runtime/bin/python|/usr/local/bin|\.zshrc|\.bashrc" src-tauri/src src-tauri/tauri.conf.json scripts .github/workflows
```

预期：生产代码和打包配置不再命中旧 runtime 路径。

### 7. 真实下载只放 ignored / nightly

真实下载测试必须存在，但不能进入普通测试。它使用真实 manifest：

```bash
cd src-tauri
RENLJ_RUNTIME_MANIFEST_URL=https://download.renlijia.com/runtimes/manifest.json \
  cargo test downloads_real_runtime_from_manifest -- --ignored --nocapture
```

这个测试证明：

```text
真实 manifest 可拉取
真实 artifact 可下载
真实 sha256 正确
真实解压成功
真实 smoke test 通过
```

### 8. 最终验证命令

后端：

```bash
cd src-tauri
cargo test \
  --test runtime_dependencies_platform_test \
  --test runtime_dependencies_paths_test \
  --test runtime_dependencies_manifest_test \
  --test runtime_dependencies_provider_test \
  --test runtime_dependencies_download_flow_test \
  --test runtime_dependencies_archive_test \
  --test runtime_dependencies_health_test \
  --test runtime_dependencies_installer_test \
  --test runtime_dependencies_mcp_placeholder_test \
  --test runtime_dependencies_python_injection_test \
  --test runtime_commands_test \
  --test runtime_dependencies_no_legacy_resource_test \
  --no-fail-fast

cargo check
```

前端：

```bash
pnpm exec vitest run src/lib/tauri.runtime.test.ts src/stores/runtimeStore.test.ts
pnpm build
```

静态搜索：

```bash
rg -n "src-tauri/python-runtime|python-runtime/bin/python|/usr/local/bin|\.zshrc|\.bashrc" src-tauri/src
```

---

## 文件结构规划

### 新增 Rust Runtime 依赖模块

- Create: `src-tauri/src/runtime/dependencies/mod.rs`
  - 对外导出 Runtime 依赖管理模块。
- Create: `src-tauri/src/runtime/dependencies/types.rs`
  - 定义 `WorkspaceDependencies`、`RuntimeDependencyError`、`RuntimeDependencyResult`。
- Create: `src-tauri/src/runtime/dependencies/platform.rs`
  - 识别 `darwin-arm64`、`darwin-x64`、`win32-x64`、`linux-x64`。
- Create: `src-tauri/src/runtime/dependencies/paths.rs`
  - 计算 app cache 下的 runtime 根目录、current 目录、downloads、staging。
- Create: `src-tauri/src/runtime/dependencies/manifest.rs`
  - 解析和校验 runtime manifest。
- Create: `src-tauri/src/runtime/dependencies/manifest_client.rs`
  - 从生产 URL / 企业镜像 / 本地测试路径读取 manifest。
- Create: `src-tauri/src/runtime/dependencies/downloader.rs`
  - 下载 runtime artifact，支持进度、重试和下载缓存。
- Create: `src-tauri/src/runtime/dependencies/provider.rs`
  - 定义 `RuntimeArtifactProvider`，支持 `OfficialSourceProvider` 和 `RenlijiaBundleProvider` 两类来源。
- Create: `src-tauri/src/runtime/dependencies/resolver.rs`
  - 对业务层暴露 `RuntimeResolver` trait 和默认实现；`mod.rs` 只负责 re-export。
- Create: `src-tauri/src/runtime/dependencies/health.rs`
  - 执行 `node -v`、`python3 --version`、`uv --version` smoke test。
- Create: `src-tauri/src/runtime/dependencies/installer.rs`
  - 负责下载、sha256、解压、staging、原子安装、current 切换。
- Create: `src-tauri/src/runtime/dependencies/archive.rs`
  - 安全解压，拒绝 Zip Slip / Tar path traversal。
- Create: `src-tauri/src/runtime/dependencies/checksum.rs`
  - sha256 文件校验。

### 修改现有 Runtime 注入点

- Modify: `src-tauri/src/runtime/mod.rs`
  - 注册 `dependencies` 模块。
- Modify: `src-tauri/src/lib.rs`
  - 在 Tauri 启动时 `app.manage(Arc<DefaultRuntimeResolver>)`。
- Modify: `src-tauri/src/runtime/tools/capability.rs`
  - `CapabilityContext` 增加窄接口访问 runtime dependencies，禁止工具自行拼路径。
- Modify: `src-tauri/src/python/runner.rs`
  - 从硬编码 Python 路径改为依赖 `RuntimeResolver`。
- Modify: `src-tauri/src/runtime/mcp/connection.rs`
  - `StdioMcpConnection::connect()` / command parsing 支持 `${renlijia.node}`、`${renlijia.python}`、`${renlijia.uvx}` 占位符解析。
- Modify: `src-tauri/src/runtime/mcp/manager.rs`
  - 仅在需要传递 resolver 给 connection 构造时调整，不在 manager 内做真实 spawn。
- Create: `src-tauri/src/transport/tauri_commands/runtime.rs`
  - 暴露 runtime health / ensure / reinstall 命令和 serde payload。
- Modify: `src-tauri/src/transport/tauri_commands/mod.rs`
  - 注册 runtime commands。

### 新增配置和 manifest

- Create: `src-tauri/runtime-manifest.test.json`
  - 测试 fixture，用于验证 schema；生产下载源由远程 manifest 或企业镜像提供。
- Create: `scripts/build-runtime-bundle.py`
  - 发布侧脚本：下载上游 runtime、整理目录、生成 artifact 和 manifest。
- Create: `docs/runtime-manager.md`
  - 产品与架构说明，记录目录、升级、企业镜像、离线包约定。
- Modify: `src-tauri/tauri.conf.json`
  - 迁移完成后移除 `python-runtime` resource 映射。
- Modify: `scripts/setup-python.sh` / `scripts/setup-python.ps1`
  - 迁移完成后停用写入 `src-tauri/python-runtime` 的旧入口，改为 runtime fixture 或删除。
- Modify: `.github/workflows/build-desktop.yml` / `.github/workflows/ci.yml`
  - 迁移完成后移除 `src-tauri/python-runtime` cache/bootstrap。

### 新增/修改前端文件

- Create: `src/lib/tauri.runtime.test.ts`
  - 测试前端 runtime Tauri API wrapper。
- Modify: `src/lib/tauri.ts`
  - 增加 `getRuntimeHealth`、`ensureRuntime`、`reinstallRuntime` wrapper。
- Create: `src/stores/runtimeStore.ts`
  - 存储 runtime 安装状态、版本、路径、下载进度、错误。
- Create: `src/stores/runtimeStore.test.ts`
  - 测试状态更新和错误展示逻辑。
- Create: `src/components/settings/RuntimeSettingsPanel.tsx`
  - 显示 Node/Python/uv 状态、路径、版本、重装按钮。
- Create: `src/components/settings/RuntimeSettingsPanel.test.tsx`
  - 测试 UI 展示与按钮行为。

### 新增测试文件

- Create: `src-tauri/tests/runtime_dependencies_platform_test.rs`
- Create: `src-tauri/tests/runtime_dependencies_paths_test.rs`
- Create: `src-tauri/tests/runtime_dependencies_manifest_test.rs`
- Create: `src-tauri/tests/runtime_dependencies_resolver_test.rs`
- Create: `src-tauri/tests/runtime_dependencies_archive_test.rs`
- Create: `src-tauri/tests/runtime_dependencies_health_test.rs`
- Create: `src-tauri/tests/runtime_dependencies_mcp_placeholder_test.rs`
- Modify: `src-tauri/tests/python_run_scope_test.rs`
  - 增加断言：PythonRunner 使用 resolver 返回的托管 Python。

---

## Phase 0：基线确认与边界锁定

### Task 0.1: 记录现状和对标结论

**Files:**
- Create: `docs/runtime-manager.md`

- [x] **Step 1: 写入架构说明文档**

Create `docs/runtime-manager.md` with:

```markdown
# Lotus Runtime Manager

Lotus Runtime Manager 负责管理应用私有 Node、Python、uv/uvx 运行环境。它的目标是让 Lotus 像 Codex/Real 一样具备可控的本地代码执行能力，而不是依赖用户系统 PATH 中的 `node`、`python` 或 `uv`。

## 目标

- Node、Python、uv 由 Lotus 托管。
- 工具执行使用绝对路径。
- Runtime 和 App 本体解耦，可以独立升级和回滚。
- 下载包必须校验 sha256。
- 用户 workspace 依赖与 Lotus 内置工具依赖隔离。
- 企业环境支持镜像源、禁用自动下载和离线导入。

## 默认目录

macOS/Linux:

```text
~/.cache/renlijia-runtimes/
  renlijia-primary-runtime/
    current -> versions/2026.04.25
    versions/
      2026.04.25/
        install.json
        dependencies/
          node/
          python/
          uv
          uvx
    downloads/
    staging/
```

Windows:

```text
%LOCALAPPDATA%\Lotus\runtimes\
  renlijia-primary-runtime\
    current
    versions\
    downloads\
    staging\
```

## 执行规则

业务代码禁止直接拼 runtime 路径，必须通过 `RuntimeResolver` 获取：

- `node`
- `npm`
- `npx`
- `python`
- `uv`
- `uvx`
- `node_modules`
- `python_site_packages`

## 非目标

- 不安装到系统目录。
- 不修改用户 shell profile。
- 不默认读取或覆盖系统 Node/Python。
- 不在本计划中引入 Docker/VM 沙箱；本计划聚焦托管 runtime，不替代后续更强隔离方案。
```

- [x] **Step 2: 检查文档没有占位符**

Run:

```bash
rg -n "占位符|未定义实现|临时方案" docs/runtime-manager.md
```

Expected: no output.

- [x] **Step 3: 提交基线文档**

```bash
git add docs/runtime-manager.md
git commit -m "docs: document runtime manager architecture"
```

---

## Phase 1：Runtime Resolver 基础能力

### Task 1.1: 新增 runtime dependency 类型

**Files:**
- Create: `src-tauri/src/runtime/dependencies/mod.rs`
- Create: `src-tauri/src/runtime/dependencies/types.rs`
- Modify: `src-tauri/src/runtime/mod.rs`
- Test: `src-tauri/tests/runtime_dependencies_resolver_test.rs`

- [x] **Step 1: 写失败测试，验证 resolver 返回绝对路径**

Create `src-tauri/tests/runtime_dependencies_resolver_test.rs`:

```rust
use std::path::PathBuf;

use app_lib::runtime::dependencies::{StaticRuntimeResolver, RuntimeResolver};

#[test]
fn static_runtime_resolver_returns_absolute_python_path() {
    let resolver = StaticRuntimeResolver::new(
        PathBuf::from("/tmp/renlijia/python/bin/python3"),
        PathBuf::from("/tmp/renlijia/node/bin/node"),
        PathBuf::from("/tmp/renlijia/node/bin/npm"),
        PathBuf::from("/tmp/renlijia/node/bin/npx"),
        PathBuf::from("/tmp/renlijia/uv"),
        PathBuf::from("/tmp/renlijia/uvx"),
        PathBuf::from("/tmp/renlijia/node/node_modules"),
        PathBuf::from("/tmp/renlijia/python/lib/python3.12/site-packages"),
    );

    let deps = resolver.workspace_dependencies().expect("dependencies should resolve");

    assert!(deps.python.is_absolute());
    assert_eq!(deps.python, PathBuf::from("/tmp/renlijia/python/bin/python3"));
    assert_eq!(deps.node, PathBuf::from("/tmp/renlijia/node/bin/node"));
    assert_eq!(deps.uv, PathBuf::from("/tmp/renlijia/uv"));
}
```

- [x] **Step 2: 运行测试确认失败**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_resolver_test -- --nocapture
```

Expected: FAIL because `runtime::dependencies` does not exist.

- [x] **Step 3: 创建类型和 trait**

Create `src-tauri/src/runtime/dependencies/types.rs`:

```rust
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceDependencies {
    pub node: PathBuf,
    pub npm: PathBuf,
    pub npx: PathBuf,
    pub python: PathBuf,
    pub uv: PathBuf,
    pub uvx: PathBuf,
    pub node_modules: PathBuf,
    pub python_site_packages: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeDependencyError {
    NonAbsolutePath { field: &'static str, path: PathBuf },
    MissingExecutable { field: &'static str, path: PathBuf },
    Io(String),
}

impl std::fmt::Display for RuntimeDependencyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonAbsolutePath { field, path } => {
                write!(f, "runtime dependency {field} must be absolute: {}", path.display())
            }
            Self::MissingExecutable { field, path } => {
                write!(f, "runtime dependency {field} is missing: {}", path.display())
            }
            Self::Io(message) => write!(f, "runtime dependency io error: {message}"),
        }
    }
}

impl std::error::Error for RuntimeDependencyError {}

pub type RuntimeDependencyResult<T> = Result<T, RuntimeDependencyError>;
```

Create `src-tauri/src/runtime/dependencies/mod.rs`:

```rust
mod types;

use std::path::PathBuf;

pub use types::{ManagedRuntimeResolver, RuntimeDependencyError, RuntimeDependencyResult, WorkspaceDependencies};

pub trait RuntimeResolver: Send + Sync {
    fn workspace_dependencies(&self) -> RuntimeDependencyResult<WorkspaceDependencies>;
}

pub type ManagedRuntimeResolver = std::sync::Arc<dyn RuntimeResolver>;

#[derive(Debug, Clone)]
pub struct StaticRuntimeResolver {
    dependencies: WorkspaceDependencies,
}

impl StaticRuntimeResolver {
    pub fn new(
        python: PathBuf,
        node: PathBuf,
        npm: PathBuf,
        npx: PathBuf,
        uv: PathBuf,
        uvx: PathBuf,
        node_modules: PathBuf,
        python_site_packages: PathBuf,
    ) -> Self {
        Self {
            dependencies: WorkspaceDependencies {
                node,
                npm,
                npx,
                python,
                uv,
                uvx,
                node_modules,
                python_site_packages,
            },
        }
    }
}

impl RuntimeResolver for StaticRuntimeResolver {
    fn workspace_dependencies(&self) -> RuntimeDependencyResult<WorkspaceDependencies> {
        validate_absolute("node", &self.dependencies.node)?;
        validate_absolute("npm", &self.dependencies.npm)?;
        validate_absolute("npx", &self.dependencies.npx)?;
        validate_absolute("python", &self.dependencies.python)?;
        validate_absolute("uv", &self.dependencies.uv)?;
        validate_absolute("uvx", &self.dependencies.uvx)?;
        Ok(self.dependencies.clone())
    }
}

fn validate_absolute(field: &'static str, path: &PathBuf) -> RuntimeDependencyResult<()> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(RuntimeDependencyError::NonAbsolutePath {
            field,
            path: path.clone(),
        })
    }
}
```

Modify `src-tauri/src/runtime/mod.rs` and add:

```rust
pub mod dependencies;
```

- [x] **Step 4: 运行测试确认通过**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_resolver_test -- --nocapture
```

Expected: PASS.

- [x] **Step 5: 提交 Resolver 基础类型**

```bash
git add src-tauri/src/runtime/dependencies src-tauri/src/runtime/mod.rs src-tauri/tests/runtime_dependencies_resolver_test.rs
git commit -m "feat: add runtime dependency resolver interface"
```

### Task 1.2: 增加平台识别

**Files:**
- Create: `src-tauri/src/runtime/dependencies/platform.rs`
- Modify: `src-tauri/src/runtime/dependencies/mod.rs`
- Test: `src-tauri/tests/runtime_dependencies_platform_test.rs`

- [x] **Step 1: 写失败测试**

Create `src-tauri/tests/runtime_dependencies_platform_test.rs`:

```rust
use app_lib::runtime::dependencies::{RuntimePlatform, RuntimePlatformError};

#[test]
fn runtime_platform_serializes_to_manifest_key() {
    assert_eq!(RuntimePlatform::DarwinArm64.manifest_key(), "darwin-arm64");
    assert_eq!(RuntimePlatform::DarwinX64.manifest_key(), "darwin-x64");
    assert_eq!(RuntimePlatform::WindowsX64.manifest_key(), "win32-x64");
    assert_eq!(RuntimePlatform::LinuxX64.manifest_key(), "linux-x64");
}

#[test]
fn runtime_platform_rejects_unknown_pair() {
    let err = RuntimePlatform::from_os_arch("freebsd", "riscv64").unwrap_err();
    assert_eq!(
        err,
        RuntimePlatformError::UnsupportedPlatform {
            os: "freebsd".to_string(),
            arch: "riscv64".to_string(),
        }
    );
}
```

- [x] **Step 2: 运行测试确认失败**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_platform_test -- --nocapture
```

Expected: FAIL because `RuntimePlatform` does not exist.

- [x] **Step 3: 实现平台识别**

Create `src-tauri/src/runtime/dependencies/platform.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimePlatform {
    DarwinArm64,
    DarwinX64,
    WindowsX64,
    LinuxX64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimePlatformError {
    UnsupportedPlatform { os: String, arch: String },
}

impl RuntimePlatform {
    pub fn current() -> Result<Self, RuntimePlatformError> {
        Self::from_os_arch(std::env::consts::OS, std::env::consts::ARCH)
    }

    pub fn from_os_arch(os: &str, arch: &str) -> Result<Self, RuntimePlatformError> {
        match (os, arch) {
            ("macos", "aarch64") => Ok(Self::DarwinArm64),
            ("macos", "x86_64") => Ok(Self::DarwinX64),
            ("windows", "x86_64") => Ok(Self::WindowsX64),
            ("linux", "x86_64") => Ok(Self::LinuxX64),
            _ => Err(RuntimePlatformError::UnsupportedPlatform {
                os: os.to_string(),
                arch: arch.to_string(),
            }),
        }
    }

    pub fn manifest_key(self) -> &'static str {
        match self {
            Self::DarwinArm64 => "darwin-arm64",
            Self::DarwinX64 => "darwin-x64",
            Self::WindowsX64 => "win32-x64",
            Self::LinuxX64 => "linux-x64",
        }
    }
}
```

Modify `src-tauri/src/runtime/dependencies/mod.rs`:

```rust
mod platform;
mod types;

pub use platform::{RuntimePlatform, RuntimePlatformError};
```

- [x] **Step 4: 运行平台测试**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_platform_test -- --nocapture
```

Expected: PASS.

- [x] **Step 5: 提交平台识别**

```bash
git add src-tauri/src/runtime/dependencies src-tauri/tests/runtime_dependencies_platform_test.rs
git commit -m "feat: add runtime platform detection"
```

### Task 1.3: 增加 runtime 路径计算

**Files:**
- Create: `src-tauri/src/runtime/dependencies/paths.rs`
- Modify: `src-tauri/src/runtime/dependencies/mod.rs`
- Test: `src-tauri/tests/runtime_dependencies_paths_test.rs`

- [x] **Step 1: 写失败测试**

Create `src-tauri/tests/runtime_dependencies_paths_test.rs`:

```rust
use std::path::PathBuf;

use app_lib::runtime::dependencies::{RuntimePathError, RuntimePaths};

#[test]
fn computes_bundle_scoped_runtime_directories() {
    let cache_root = std::env::temp_dir().join("renlijia-runtimes");
    let paths = RuntimePaths::new(cache_root.clone(), "renlijia-primary-runtime")
        .expect("valid runtime paths");

    let bundle_root = cache_root.join("renlijia-primary-runtime");
    assert_eq!(paths.bundle_root(), bundle_root);
    assert_eq!(paths.current_dir(), bundle_root.join("current"));
    assert_eq!(paths.versions_dir(), bundle_root.join("versions"));
    assert_eq!(paths.downloads_dir(), bundle_root.join("downloads"));
    assert_eq!(paths.staging_dir(), bundle_root.join("staging"));
}

#[test]
fn computes_version_directory_under_versions_layout() {
    let cache_root = std::env::temp_dir().join("renlijia-runtimes");
    let paths = RuntimePaths::new(cache_root.clone(), "renlijia-primary-runtime")
        .expect("valid runtime paths");

    assert_eq!(
        paths.version_dir("2026.04.25").expect("valid version"),
        cache_root
            .join("renlijia-primary-runtime")
            .join("versions")
            .join("2026.04.25")
    );
}

#[test]
fn rejects_relative_cache_root() {
    let error = RuntimePaths::new(PathBuf::from("relative-cache"), "renlijia-primary-runtime")
        .unwrap_err();

    assert_eq!(
        error,
        RuntimePathError::NonAbsoluteCacheRoot {
            path: PathBuf::from("relative-cache"),
        }
    );
}

#[test]
fn rejects_bundle_id_that_is_not_a_safe_path_segment() {
    let cache_root = std::env::temp_dir().join("renlijia-runtimes");

    for bundle_id in ["", "../escape", "nested/bundle", "."] {
        let error = RuntimePaths::new(cache_root.clone(), bundle_id).unwrap_err();
        assert_eq!(
            error,
            RuntimePathError::UnsafePathSegment {
                field: "bundle_id",
                value: bundle_id.to_string(),
            }
        );
    }
}

#[test]
fn rejects_version_that_is_not_a_safe_path_segment() {
    let cache_root = std::env::temp_dir().join("renlijia-runtimes");
    let paths = RuntimePaths::new(cache_root, "renlijia-primary-runtime")
        .expect("valid runtime paths");

    for version in ["", "../escape", "nested/version", "."] {
        let error = paths.version_dir(version).unwrap_err();
        assert_eq!(
            error,
            RuntimePathError::UnsafePathSegment {
                field: "version",
                value: version.to_string(),
            }
        );
    }
}
```

- [x] **Step 2: 运行测试确认失败**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_paths_test -- --nocapture
```

Expected: FAIL because `RuntimePaths` / `RuntimePathError` does not exist.

- [x] **Step 3: 实现安全路径对象**

Create `src-tauri/src/runtime/dependencies/paths.rs`:

```rust
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePaths {
    cache_root: PathBuf,
    bundle_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimePathError {
    NonAbsoluteCacheRoot { path: PathBuf },
    UnsafePathSegment { field: &'static str, value: String },
}

impl RuntimePaths {
    pub fn new(cache_root: PathBuf, bundle_id: impl Into<String>) -> Result<Self, RuntimePathError> {
        if !cache_root.is_absolute() {
            return Err(RuntimePathError::NonAbsoluteCacheRoot { path: cache_root });
        }
        let bundle_id = bundle_id.into();
        validate_safe_path_segment("bundle_id", &bundle_id)?;
        Ok(Self { cache_root, bundle_id })
    }

    pub fn bundle_root(&self) -> PathBuf {
        self.cache_root.join(&self.bundle_id)
    }

    pub fn current_dir(&self) -> PathBuf {
        self.bundle_root().join("current")
    }

    pub fn versions_dir(&self) -> PathBuf {
        self.bundle_root().join("versions")
    }

    pub fn downloads_dir(&self) -> PathBuf {
        self.bundle_root().join("downloads")
    }

    pub fn staging_dir(&self) -> PathBuf {
        self.bundle_root().join("staging")
    }

    pub fn version_dir(&self, version: impl AsRef<str>) -> Result<PathBuf, RuntimePathError> {
        let version = version.as_ref();
        validate_safe_path_segment("version", version)?;
        Ok(self.versions_dir().join(version))
    }
}

fn validate_safe_path_segment(field: &'static str, value: &str) -> Result<(), RuntimePathError> {
    let components = Path::new(value).components().collect::<Vec<_>>();
    if matches!(components.as_slice(), [Component::Normal(_)]) {
        Ok(())
    } else {
        Err(RuntimePathError::UnsafePathSegment { field, value: value.to_string() })
    }
}
```

Modify `src-tauri/src/runtime/dependencies/mod.rs`:

```rust
mod paths;
pub use paths::{RuntimePathError, RuntimePaths};
```

- [x] **Step 4: 运行路径测试**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_paths_test -- --nocapture
```

Expected: PASS.

- [x] **Step 5: 提交路径计算**

```bash
git add src-tauri/src/runtime/dependencies src-tauri/tests/runtime_dependencies_paths_test.rs
git commit -m "feat: add runtime path calculation"
```

### Task 2.1: 增加生产 manifest schema 与测试 fixture

**Files:**
- Create: `src-tauri/runtime-manifest.test.json`
- Create: `src-tauri/src/runtime/dependencies/manifest.rs`
- Modify: `src-tauri/src/runtime/dependencies/mod.rs`
- Test: `src-tauri/tests/runtime_dependencies_manifest_test.rs`

- [x] **Step 1: 写失败测试**

Create `src-tauri/tests/runtime_dependencies_manifest_test.rs`:

```rust
use app_lib::runtime::dependencies::{RuntimeManifest, RuntimePlatform};

#[test]
fn manifest_selects_node_for_current_platform_key() {
    let json = r#"
    {
      "bundleVersion": "2026.04.25",
      "source": "test-fixture",
      "runtimes": {
        "node": {
          "version": "22.19.0",
          "platforms": {
            "darwin-arm64": {
              "url": "https://download.renlijia.com/runtimes/test-fixtures/node.tar.gz",
              "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }
          }
        }
      }
    }
    "#;

    let manifest = RuntimeManifest::from_json(json).expect("manifest should parse");
    let artifact = manifest
        .artifact("node", RuntimePlatform::DarwinArm64)
        .expect("node artifact should exist");

    assert_eq!(artifact.url, "https://download.renlijia.com/runtimes/test-fixtures/node.tar.gz");
    assert_eq!(artifact.sha256.len(), 64);
}
```

- [x] **Step 2: 运行测试确认失败**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_manifest_test -- --nocapture
```

Expected: FAIL because `RuntimeManifest` does not exist.

- [x] **Step 3: 实现 manifest 解析**

Create `src-tauri/src/runtime/dependencies/manifest.rs`:

```rust
use std::collections::BTreeMap;

use serde::Deserialize;

use super::RuntimePlatform;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeManifest {
    pub bundle_version: String,
    pub source: String,
    pub runtimes: BTreeMap<String, RuntimeSpec>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RuntimeSpec {
    pub version: String,
    pub platforms: BTreeMap<String, RuntimeArtifact>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RuntimeArtifact {
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeManifestError {
    Json(String),
    MissingRuntime { name: String, platform: String },
    InvalidSha256 { name: String, sha256: String },
    EmptyRuntimes,
    EmptyPlatforms { name: String },
    UntrustedArtifactUrl { name: String, platform: String, url: String },
}

impl RuntimeManifest {
    pub fn from_json(input: &str) -> Result<Self, RuntimeManifestError> {
        let manifest: Self = serde_json::from_str(input)
            .map_err(|err| RuntimeManifestError::Json(err.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn artifact(
        &self,
        name: &str,
        platform: RuntimePlatform,
    ) -> Result<&RuntimeArtifact, RuntimeManifestError> {
        let key = platform.manifest_key();
        self.runtimes
            .get(name)
            .and_then(|runtime| runtime.platforms.get(key))
            .ok_or_else(|| RuntimeManifestError::MissingRuntime {
                name: name.to_string(),
                platform: key.to_string(),
            })
    }

    fn validate(&self) -> Result<(), RuntimeManifestError> {
        if self.runtimes.is_empty() {
            return Err(RuntimeManifestError::EmptyRuntimes);
        }

        for (name, runtime) in &self.runtimes {
            if runtime.platforms.is_empty() {
                return Err(RuntimeManifestError::EmptyPlatforms { name: name.clone() });
            }

            for (platform, artifact) in &runtime.platforms {
                let valid = artifact.sha256.len() == 64
                    && artifact.sha256.chars().all(|ch| ch.is_ascii_hexdigit());
                if !valid {
                    return Err(RuntimeManifestError::InvalidSha256 {
                        name: name.clone(),
                        sha256: artifact.sha256.clone(),
                    });
                }
                if !artifact.url.starts_with("https://")
                    || artifact.url.contains("localhost")
                    || artifact.url.contains("127.0.0.1")
                {
                    return Err(RuntimeManifestError::UntrustedArtifactUrl {
                        name: name.clone(),
                        platform: platform.clone(),
                        url: artifact.url.clone(),
                    });
                }
            }
        }
        Ok(())
    }
}
```

Modify `src-tauri/src/runtime/dependencies/mod.rs`:

```rust
mod manifest;
pub use manifest::{RuntimeArtifact, RuntimeManifest, RuntimeManifestError, RuntimeSpec};
```

Create `src-tauri/runtime-manifest.test.json`:

```json
{
  "bundleVersion": "2026.04.25",
  "source": "test-fixture",
  "runtimes": {
    "node": {
      "version": "22.19.0",
      "platforms": {
        "darwin-arm64": {
          "url": "https://download.renlijia.com/runtimes/test-fixtures/node-v22.19.0-darwin-arm64.tar.gz",
          "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        }
      }
    },
    "python": {
      "version": "3.12.13",
      "platforms": {
        "darwin-arm64": {
          "url": "https://download.renlijia.com/runtimes/test-fixtures/python-3.12.13-darwin-arm64.tar.gz",
          "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        }
      }
    },
    "uv": {
      "version": "0.7.13",
      "platforms": {
        "darwin-arm64": {
          "url": "https://download.renlijia.com/runtimes/test-fixtures/uv-0.7.13-darwin-arm64.tar.gz",
          "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
        }
      }
    }
  }
}
```

- [x] **Step 4: 运行 manifest 测试**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_manifest_test -- --nocapture
```

Expected: PASS.

- [x] **Step 5: 提交 manifest schema**

```bash
git add src-tauri/runtime-manifest.test.json src-tauri/src/runtime/dependencies src-tauri/tests/runtime_dependencies_manifest_test.rs
git commit -m "feat: add runtime manifest schema"
```

### Task 2.2: 增加 manifest client 与 artifact downloader

**Files:**
- Create: `src-tauri/src/runtime/dependencies/manifest_client.rs`
- Create: `src-tauri/src/runtime/dependencies/downloader.rs`
- Modify: `src-tauri/src/runtime/dependencies/mod.rs`
- Test: `src-tauri/tests/runtime_dependencies_download_flow_test.rs`

- [x] **Step 1: 写失败测试，验证下载流程不依赖 shell 脚本**

Create `src-tauri/tests/runtime_dependencies_download_flow_test.rs`:

```rust
use app_lib::runtime::dependencies::{RuntimeDownloadPlan, RuntimeManifestSource};

#[test]
fn runtime_download_plan_uses_manifest_and_artifact_url_not_shell_script() {
    let plan = RuntimeDownloadPlan::new(
        RuntimeManifestSource::Url("https://download.renlijia.com/runtimes/manifest.json".to_string()),
        "darwin-arm64".to_string(),
    );

    assert_eq!(
        plan.manifest_source().as_url(),
        Some("https://download.renlijia.com/runtimes/manifest.json")
    );
    assert_eq!(plan.platform(), "darwin-arm64");
    assert!(!plan.uses_shell_script());
}
```

- [x] **Step 2: 运行测试确认失败**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_download_flow_test -- --nocapture
```

Expected: FAIL because `RuntimeDownloadPlan` and `RuntimeManifestSource` do not exist.

- [x] **Step 3: 实现 manifest source 和 download plan 类型**

Create `src-tauri/src/runtime/dependencies/manifest_client.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeManifestSource {
    Url(String),
    File(std::path::PathBuf),
}

impl RuntimeManifestSource {
    pub fn as_url(&self) -> Option<&str> {
        match self {
            Self::Url(url) => Some(url.as_str()),
            Self::File(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDownloadPlan {
    manifest_source: RuntimeManifestSource,
    platform: String,
}

impl RuntimeDownloadPlan {
    pub fn new(manifest_source: RuntimeManifestSource, platform: String) -> Self {
        Self { manifest_source, platform }
    }

    pub fn manifest_source(&self) -> &RuntimeManifestSource {
        &self.manifest_source
    }

    pub fn platform(&self) -> &str {
        &self.platform
    }

    pub fn uses_shell_script(&self) -> bool {
        false
    }
}
```

Create `src-tauri/src/runtime/dependencies/downloader.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeDownloadError {
    Network(String),
    Io(String),
    InvalidStatus(u16),
}
```

Modify `src-tauri/src/runtime/dependencies/mod.rs`:

```rust
mod downloader;
mod manifest_client;

pub use downloader::{RuntimeDownloadError, RuntimeDownloadProgress};
pub use manifest_client::{RuntimeDownloadPlan, RuntimeManifestSource};
```

- [x] **Step 4: 运行下载流程测试**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_download_flow_test -- --nocapture
```

Expected: PASS.

- [x] **Step 5: 提交 manifest client / downloader 骨架**

```bash
git add src-tauri/src/runtime/dependencies src-tauri/tests/runtime_dependencies_download_flow_test.rs
git commit -m "feat: add runtime manifest client download plan"
```

### Task 2.3: 增加 RuntimeArtifactProvider 抽象

**Files:**
- Create: `src-tauri/src/runtime/dependencies/provider.rs`
- Modify: `src-tauri/src/runtime/dependencies/mod.rs`
- Test: `src-tauri/tests/runtime_dependencies_provider_test.rs`

- [x] **Step 1: 写失败测试，验证 provider 不硬编码公网 URL**

Create `src-tauri/tests/runtime_dependencies_provider_test.rs`:

```rust
use app_lib::runtime::dependencies::{RuntimeArtifactProviderKind, RuntimeArtifactProviderPolicy};

#[test]
fn provider_policy_allows_official_sources_only_through_manifest() {
    let policy = RuntimeArtifactProviderPolicy::new(RuntimeArtifactProviderKind::OfficialSource);

    assert!(policy.requires_manifest_url());
    assert!(!policy.allows_hardcoded_upstream_urls());
}

#[test]
fn renlijia_bundle_provider_is_the_production_default() {
    let policy = RuntimeArtifactProviderPolicy::production_default();

    assert_eq!(policy.kind(), RuntimeArtifactProviderKind::RenlijiaBundle);
}
```

- [x] **Step 2: 运行测试确认失败**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_provider_test -- --nocapture
```

Expected: FAIL because provider types do not exist.

- [x] **Step 3: 实现 provider 类型**

Create `src-tauri/src/runtime/dependencies/provider.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeArtifactProviderKind {
    OfficialSource,
    RenlijiaBundle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeArtifactProviderPolicy {
    kind: RuntimeArtifactProviderKind,
}

impl RuntimeArtifactProviderPolicy {
    pub fn new(kind: RuntimeArtifactProviderKind) -> Self {
        Self { kind }
    }

    pub fn production_default() -> Self {
        Self {
            kind: RuntimeArtifactProviderKind::RenlijiaBundle,
        }
    }

    pub fn kind(&self) -> RuntimeArtifactProviderKind {
        self.kind
    }

    pub fn requires_manifest_url(&self) -> bool {
        true
    }

    pub fn allows_hardcoded_upstream_urls(&self) -> bool {
        false
    }
}
```

Modify `src-tauri/src/runtime/dependencies/mod.rs`:

```rust
mod provider;
pub use provider::{RuntimeArtifactProviderKind, RuntimeArtifactProviderPolicy};
```

- [x] **Step 4: 运行 provider 测试**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_provider_test -- --nocapture
```

Expected: PASS.

- [x] **Step 5: 提交 provider 抽象**

```bash
git add src-tauri/src/runtime/dependencies src-tauri/tests/runtime_dependencies_provider_test.rs
git commit -m "feat: add runtime artifact provider policy"
```

### Task 2.4: 增加 health smoke test

**Files:**
- Create: `src-tauri/src/runtime/dependencies/health.rs`
- Modify: `src-tauri/src/runtime/dependencies/mod.rs`
- Test: `src-tauri/tests/runtime_dependencies_health_test.rs`

- [x] **Step 1: 写失败测试，使用 shell stub 模拟 node/python/uv**

Create `src-tauri/tests/runtime_dependencies_health_test.rs`:

```rust
use std::fs;
use std::os::unix::fs::PermissionsExt;

use tempfile::tempdir;

use app_lib::runtime::dependencies::{RuntimeHealthChecker, RuntimeToolProbe};

#[test]
fn runtime_health_checker_reads_versions_from_executables() {
    let dir = tempdir().expect("tempdir");
    let node = dir.path().join("node");
    let python = dir.path().join("python3");
    let uv = dir.path().join("uv");

    write_executable(&node, "#!/bin/sh\necho 'v22.19.0'\n");
    write_executable(&python, "#!/bin/sh\necho 'Python 3.12.13'\n");
    write_executable(&uv, "#!/bin/sh\necho 'uv 0.7.13'\n");

    let checker = RuntimeHealthChecker::default();
    let report = checker
        .check(&[
            RuntimeToolProbe::new("node", node),
            RuntimeToolProbe::new("python", python),
            RuntimeToolProbe::new("uv", uv),
        ])
        .expect("health check should pass");

    assert_eq!(report.tool_version("node"), Some("v22.19.0"));
    assert_eq!(report.tool_version("python"), Some("Python 3.12.13"));
    assert_eq!(report.tool_version("uv"), Some("uv 0.7.13"));
}

fn write_executable(path: &std::path::Path, contents: &str) {
    fs::write(path, contents).expect("write stub");
    let mut permissions = fs::metadata(path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("chmod");
}
```

- [x] **Step 2: 运行测试确认失败**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_health_test -- --nocapture
```

Expected: FAIL because `RuntimeHealthChecker` does not exist.

- [x] **Step 3: 实现 health checker**

Create `src-tauri/src/runtime/dependencies/health.rs`:

```text
Implementation requirements:
- Define RuntimeToolProbe, RuntimeHealthReport, RuntimeHealthError, and RuntimeHealthChecker.
- RuntimeHealthChecker::default() uses a bounded timeout, currently 5 seconds.
- RuntimeHealthChecker::with_timeout(timeout) is available for tests and callers that need a stricter bound.
- check() executes each probe with --version, captures stdout/stderr, and trims stdout as the version string.
- Startup failure or non-zero exit returns RuntimeHealthError::CommandFailed with the tool name.
- Timeout returns RuntimeHealthError::CommandTimedOut with the tool name and timeout_ms.
- On Unix, spawn the probe in its own process group using pre_exec + setpgid(0, 0), then killpg(pid, SIGKILL) and wait on timeout.
- On non-Unix, kill the direct child and wait on timeout.
- RuntimeHealthError implements Display and std::error::Error.
```

Modify `src-tauri/src/runtime/dependencies/mod.rs`:

```rust
mod health;
pub use health::{RuntimeHealthChecker, RuntimeHealthError, RuntimeHealthReport, RuntimeToolProbe};
```

- [x] **Step 4: 运行 health 测试**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_health_test -- --nocapture
```

Expected: PASS.

- [x] **Step 5: 提交 health checker**

```bash
git add src-tauri/src/runtime/dependencies src-tauri/tests/runtime_dependencies_health_test.rs
git commit -m "feat: add runtime health checks"
```

---

## Phase 3：安全解压、校验和安装器

### Task 3.1: 增加 sha256 校验

**Files:**
- Create: `src-tauri/src/runtime/dependencies/checksum.rs`
- Modify: `src-tauri/src/runtime/dependencies/mod.rs`
- Test: `src-tauri/tests/runtime_dependencies_checksum_test.rs`

- [x] **Step 1: 写失败测试**

Create `src-tauri/tests/runtime_dependencies_checksum_test.rs`:

```rust
use std::fs;

use tempfile::tempdir;

use app_lib::runtime::dependencies::verify_sha256;

#[test]
fn verify_sha256_accepts_matching_file() {
    let dir = tempdir().expect("tempdir");
    let file = dir.path().join("artifact.txt");
    fs::write(&file, b"renlijia-runtime").expect("write artifact");

    let expected = "61a0ff6558f97b0a562a490e0d2d0aa96fb4c46f7a05301a2a9dcb99e4de99a7";

    verify_sha256(&file, expected).expect("checksum should match");
}

#[test]
fn verify_sha256_rejects_mismatch() {
    let dir = tempdir().expect("tempdir");
    let file = dir.path().join("artifact.txt");
    fs::write(&file, b"renlijia-runtime").expect("write artifact");

    let err = verify_sha256(&file, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .expect_err("checksum should fail");

    assert!(err.to_string().contains("sha256 mismatch"));
}
```

- [x] **Step 2: 运行测试确认失败**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_checksum_test -- --nocapture
```

Expected: FAIL because `verify_sha256` does not exist.

- [x] **Step 3: 实现 sha256 校验**

Create `src-tauri/src/runtime/dependencies/checksum.rs`:

```rust
use std::fs::File;
use std::io::{Read, BufReader};
use std::path::Path;

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChecksumError {
    Io(String),
    InvalidExpected { expected: String },
    Mismatch { expected: String, actual: String },
}

impl std::fmt::Display for ChecksumError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) => write!(f, "checksum io error: {message}"),
            Self::Mismatch { expected, actual } => {
                write!(f, "sha256 mismatch: expected {expected}, actual {actual}")
            }
        }
    }
}

impl std::error::Error for ChecksumError {}

pub fn verify_sha256(path: &Path, expected: &str) -> Result<(), ChecksumError> {
    let file = File::open(path).map_err(|err| ChecksumError::Io(err.to_string()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];

    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|err| ChecksumError::Io(err.to_string()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    let actual = format!("{:x}", hasher.finalize());
    if actual == expected.to_ascii_lowercase() {
        Ok(())
    } else {
        Err(ChecksumError::Mismatch {
            expected: expected.to_string(),
            actual,
        })
    }
}
```

Modify `src-tauri/src/runtime/dependencies/mod.rs`:

```rust
mod checksum;
pub use checksum::{verify_sha256, ChecksumError};
```

如果尚未引入 `sha2`，Modify `src-tauri/Cargo.toml`:

```toml
sha2 = "0.10"
```

- [x] **Step 4: 运行 checksum 测试**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_checksum_test -- --nocapture
```

Expected: PASS.

- [x] **Step 5: 提交 checksum**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/runtime/dependencies src-tauri/tests/runtime_dependencies_checksum_test.rs
git commit -m "feat: add runtime artifact checksum verification"
```

### Task 3.2: 增加安全解压边界

安全边界要求：archive entry 必须是安全相对路径；拒绝空 entry、`.`、`..`、绝对路径、parent traversal、Windows 反斜杠路径和平台 prefix。真实解压任务必须先调用该函数，再写入 staging。

**Files:**
- Create: `src-tauri/src/runtime/dependencies/archive.rs`
- Modify: `src-tauri/src/runtime/dependencies/mod.rs`
- Test: `src-tauri/tests/runtime_dependencies_archive_test.rs`

- [x] **Step 1: 写 path traversal 失败测试**

Create `src-tauri/tests/runtime_dependencies_archive_test.rs`:

```rust
use std::path::PathBuf;

use app_lib::runtime::dependencies::validate_archive_entry_path;

#[test]
fn archive_entry_path_rejects_parent_traversal() {
    let dest = PathBuf::from("/tmp/renlijia-runtime/staging");
    let err = validate_archive_entry_path(&dest, "../evil.sh")
        .expect_err("parent traversal must be rejected");

    assert!(err.to_string().contains("unsafe archive entry"));
}

#[test]
fn archive_entry_path_accepts_nested_relative_file() {
    let dest = PathBuf::from("/tmp/renlijia-runtime/staging");
    let path = validate_archive_entry_path(&dest, "node/bin/node")
        .expect("nested path should be accepted");

    assert_eq!(path, PathBuf::from("/tmp/renlijia-runtime/staging/node/bin/node"));
}
```

- [x] **Step 2: 运行测试确认失败**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_archive_test -- --nocapture
```

Expected: FAIL because `validate_archive_entry_path` does not exist.

- [x] **Step 3: 实现 path 校验**

Create `src-tauri/src/runtime/dependencies/archive.rs`:

```rust
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveError {
    UnsafeEntry { entry: String },
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsafeEntry { entry } => write!(f, "unsafe archive entry: {entry}"),
        }
    }
}

impl std::error::Error for ArchiveError {}

pub fn validate_archive_entry_path(dest: &Path, entry: &str) -> Result<PathBuf, ArchiveError> {
    let entry_path = Path::new(entry);
    if entry_path.is_absolute() {
        return Err(ArchiveError::UnsafeEntry {
            entry: entry.to_string(),
        });
    }

    for component in entry_path.components() {
        match component {
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                return Err(ArchiveError::UnsafeEntry {
                    entry: entry.to_string(),
                });
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }

    Ok(dest.join(entry_path))
}
```

Modify `src-tauri/src/runtime/dependencies/mod.rs`:

```rust
mod archive;
pub use archive::{validate_archive_entry_path, ArchiveError};
```

- [x] **Step 4: 运行 archive 测试**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_archive_test -- --nocapture
```

Expected: PASS.

- [x] **Step 5: 提交安全解压边界**

```bash
git add src-tauri/src/runtime/dependencies src-tauri/tests/runtime_dependencies_archive_test.rs
git commit -m "feat: guard runtime archive extraction paths"
```

### Task 3.3: 增加 installer 状态机骨架

**Files:**
- Create: `src-tauri/src/runtime/dependencies/installer.rs`
- Modify: `src-tauri/src/runtime/dependencies/mod.rs`
- Test: `src-tauri/tests/runtime_dependencies_installer_test.rs`

Installer 只允许写 `downloads/`、`staging/`、`versions/` 和 `current`，不能执行系统安装，不能写 `/usr/local/bin`，不能修改 PATH。

状态机安全要求：
- `current` 是指针文件，内容为 `versions/<bundle_version>`，不能当目录使用。
- 全新安装先写 `staging/<version>`，再 promote 到 `versions/<version>`，最后原子替换 `current` 指针。
- 已存在的 `versions/<version>` 不能先删后装；缺 manifest 时只能补 `install.json`，必须保留已有 runtime payload。
- `current` 指针写入必须通过临时文件加原子替换；Windows 使用 `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`，Unix 使用 `rename`。
- 若全新版本 promote 后 `current` 替换失败，必须回滚本次新建的 version 目录，不能让 `current` 指向不一致状态。

- [x] **Step 1: 写失败测试：已安装版本通过 current 指针跳过安装**

Create `src-tauri/tests/runtime_dependencies_installer_test.rs`:

```rust
use std::fs;

use tempfile::tempdir;

use app_lib::runtime::dependencies::{RuntimeInstallPlan, RuntimeInstaller, RuntimePaths};

#[test]
fn installer_skips_when_current_points_to_matching_version_dir() {
    let dir = tempdir().expect("tempdir");
    let paths = RuntimePaths::new(dir.path().to_path_buf(), "renlijia-primary-runtime")
        .expect("valid runtime paths");
    let version_dir = paths.version_dir("2026.04.25").expect("valid version");
    fs::create_dir_all(&version_dir).expect("create version dir");
    fs::write(
        version_dir.join("install.json"),
        r#"{"bundleVersion":"2026.04.25"}"#,
    )
    .expect("write install json");
    fs::write(paths.bundle_root().join("current"), "versions/2026.04.25")
        .expect("write current pointer");

    let installer = RuntimeInstaller::new(paths);
    let result = installer
        .ensure(RuntimeInstallPlan::already_local("2026.04.25"))
        .expect("ensure should pass");

    assert!(result.skipped);
    assert_eq!(result.bundle_version, "2026.04.25");
    assert_eq!(result.install_dir, version_dir);
}
```

- [x] **Step 2: 运行测试确认失败**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_installer_test -- --nocapture
```

Expected: FAIL because installer does not exist.

- [x] **Step 3: 实现 installer 骨架，锁定 versions/current pointer 语义**

Create `src-tauri/src/runtime/dependencies/installer.rs`:

```rust
use std::fs;
use std::path::PathBuf;

use super::RuntimePaths;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInstallPlan {
    pub bundle_version: String,
}

impl RuntimeInstallPlan {
    pub fn already_local(bundle_version: impl Into<String>) -> Self {
        Self {
            bundle_version: bundle_version.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInstallResult {
    pub bundle_version: String,
    pub install_dir: PathBuf,
    pub skipped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeInstallError {
    Io(String),
    InvalidPath(String),
}

impl std::fmt::Display for RuntimeInstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) => write!(f, "runtime installer io error: {message}"),
            Self::InvalidPath(message) => write!(f, "runtime installer path error: {message}"),
        }
    }
}

impl std::error::Error for RuntimeInstallError {}

#[derive(Debug, Clone)]
pub struct RuntimeInstaller {
    paths: RuntimePaths,
}

impl RuntimeInstaller {
    pub fn new(paths: RuntimePaths) -> Self {
        Self { paths }
    }

    pub fn ensure(&self, plan: RuntimeInstallPlan) -> Result<RuntimeInstallResult, RuntimeInstallError> {
        let install_dir = self
            .paths
            .version_dir(&plan.bundle_version)
            .map_err(|error| RuntimeInstallError::InvalidPath(error.to_string()))?;
        let current_pointer = self.paths.bundle_root().join("current");
        let expected_pointer = format!("versions/{}", plan.bundle_version);
        let existing_pointer = fs::read_to_string(&current_pointer).unwrap_or_default();
        let existing_install_json = fs::read_to_string(install_dir.join("install.json")).unwrap_or_default();
        let expected_json = format!("\"bundleVersion\":\"{}\"", plan.bundle_version);

        if existing_pointer.trim() == expected_pointer && existing_install_json.contains(&expected_json) {
            return Ok(RuntimeInstallResult {
                bundle_version: plan.bundle_version,
                install_dir,
                skipped: true,
            });
        }

        fs::create_dir_all(&install_dir).map_err(|err| RuntimeInstallError::Io(err.to_string()))?;
        fs::write(
            install_dir.join("install.json"),
            format!(r#"{{"bundleVersion":"{}"}}"#, plan.bundle_version),
        )
        .map_err(|err| RuntimeInstallError::Io(err.to_string()))?;
        fs::write(current_pointer, expected_pointer)
            .map_err(|err| RuntimeInstallError::Io(err.to_string()))?;

        Ok(RuntimeInstallResult {
            bundle_version: plan.bundle_version,
            install_dir,
            skipped: false,
        })
    }
}
```

Modify `src-tauri/src/runtime/dependencies/mod.rs`:

```rust
mod installer;
pub use installer::{RuntimeInstallError, RuntimeInstallPlan, RuntimeInstallResult, RuntimeInstaller};
```

- [x] **Step 4: 运行 installer 测试**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_installer_test -- --nocapture
```

Expected: PASS.

- [x] **Step 5: 提交 installer 骨架**

```bash
git add src-tauri/src/runtime/dependencies src-tauri/tests/runtime_dependencies_installer_test.rs
git commit -m "feat: add runtime installer state skeleton"
```

---

## Phase 4：接入 PythonRunner 与工具能力上下文

### Task 4.1: CapabilityContext 暴露 runtime dependencies 窄接口

本任务只建立工具层可调用的窄接口：`CapabilityContext::workspace_dependencies()` 和可选 `runtime_resolver` 字段。生产执行路径暂时允许填 `None`，但这不是最终状态；Task 4.3 必须把 registry/query_engine/worker runtime 的构造链路接入真实 resolver，否则工具调用该接口会返回 `ResolverUnavailable`。

**Files:**
- Modify: `src-tauri/src/runtime/tools/capability.rs`
- Test: `src-tauri/tests/tool_capability_context_test.rs`

- [x] **Step 1: 写失败测试，断言 capability 可以返回 dependencies**

Append to `src-tauri/tests/tool_capability_context_test.rs`:

```rust
#[test]
fn capability_context_exposes_runtime_dependencies() {
    use std::path::PathBuf;
    use std::sync::Arc;

    use app_lib::runtime::dependencies::{RuntimeResolver, StaticRuntimeResolver};
    use app_lib::runtime::tools::capability::CapabilityContext;

    let resolver: Arc<dyn RuntimeResolver> = Arc::new(StaticRuntimeResolver::new(
        PathBuf::from("/tmp/renlijia/python/bin/python3"),
        PathBuf::from("/tmp/renlijia/node/bin/node"),
        PathBuf::from("/tmp/renlijia/node/bin/npm"),
        PathBuf::from("/tmp/renlijia/node/bin/npx"),
        PathBuf::from("/tmp/renlijia/uv"),
        PathBuf::from("/tmp/renlijia/uvx"),
        PathBuf::from("/tmp/renlijia/node/node_modules"),
        PathBuf::from("/tmp/renlijia/python/lib/python3.12/site-packages"),
    ));

    let context = CapabilityContext::for_tests_with_runtime_resolver(resolver);
    let deps = context.workspace_dependencies().expect("deps should resolve");

    assert_eq!(deps.python, PathBuf::from("/tmp/renlijia/python/bin/python3"));
}
```

- [x] **Step 2: 运行测试确认失败**

Run:

```bash
cd src-tauri && cargo test --test tool_capability_context_test capability_context_exposes_runtime_dependencies -- --nocapture
```

Expected: FAIL because `for_tests_with_runtime_resolver` or `workspace_dependencies` does not exist.

- [x] **Step 3: 修改 CapabilityContext，使用可选 resolver 和 builder**

Modify `src-tauri/src/runtime/tools/capability.rs` by adding one optional field to the existing struct. Do not make the resolver mandatory, because many existing tests and tool contexts do not need runtime dependencies.

```rust
use std::sync::Arc;

use crate::runtime::dependencies::{RuntimeDependencyError, RuntimeDependencyResult, RuntimeResolver, WorkspaceDependencies};

pub struct CapabilityContext {
    pub runtime_resolver: Option<Arc<dyn RuntimeResolver>>,
    // existing fields remain as they are today:
    // storage, workspace_id, browser_available, file_ops, read_file_state,
    // file_reading_limits, notification_sink, is_subagent
}

impl CapabilityContext {
    pub fn with_runtime_resolver(mut self, runtime_resolver: Arc<dyn RuntimeResolver>) -> Self {
        self.runtime_resolver = Some(runtime_resolver);
        self
    }

    pub fn workspace_dependencies(&self) -> RuntimeDependencyResult<WorkspaceDependencies> {
        self.runtime_resolver
            .as_ref()
            .ok_or_else(|| RuntimeDependencyError::Io("runtime resolver is not configured".to_string()))?
            .workspace_dependencies()
    }

    #[cfg(test)]
    pub fn for_tests_with_runtime_resolver(runtime_resolver: Arc<dyn RuntimeResolver>) -> Self {
        Self::with_workspace(std::path::PathBuf::from("/tmp/renlijia/workspace"), "test-workspace")
            .with_runtime_resolver(runtime_resolver)
    }
}
```

Also update the existing `CapabilityContext::with_workspace(...)` constructor so the new field is initialized as `runtime_resolver: None`. Update any direct struct literal in tests by adding `runtime_resolver: None`; do not change call sites that can continue using the builder.

- [x] **Step 4: 运行 capability 测试**

Run:

```bash
cd src-tauri && cargo test --test tool_capability_context_test capability_context_exposes_runtime_dependencies -- --nocapture
```

Expected: PASS.

- [x] **Step 5: 提交 capability runtime 注入**

```bash
git add src-tauri/src/runtime/tools/capability.rs src-tauri/tests/tool_capability_context_test.rs
git commit -m "feat: expose runtime dependencies through capability context"
```

### Task 4.2: PythonRunner 使用 RuntimeResolver

**Files:**
- Modify: `src-tauri/src/python/runner.rs`
- Modify: `src-tauri/tests/python_run_scope_test.rs`

- [x] **Step 1: 写失败测试，断言 runner 调用 resolver 路径**

Append to `src-tauri/tests/python_run_scope_test.rs`:

```rust
#[test]
fn python_runner_uses_managed_python_from_runtime_resolver() {
    use std::path::PathBuf;
    use std::sync::Arc;

    use app_lib::python::runner::PythonRunner;
    use app_lib::runtime::dependencies::StaticRuntimeResolver;

    let resolver = Arc::new(StaticRuntimeResolver::new(
        PathBuf::from("/tmp/managed-python/bin/python3"),
        PathBuf::from("/tmp/managed-node/bin/node"),
        PathBuf::from("/tmp/managed-node/bin/npm"),
        PathBuf::from("/tmp/managed-node/bin/npx"),
        PathBuf::from("/tmp/managed-uv"),
        PathBuf::from("/tmp/managed-uvx"),
        PathBuf::from("/tmp/managed-node/node_modules"),
        PathBuf::from("/tmp/managed-python/site-packages"),
    ));

    let runner = PythonRunner::for_tests_with_runtime_resolver(resolver);

    assert_eq!(
        runner.python_executable_for_tests(),
        PathBuf::from("/tmp/managed-python/bin/python3")
    );
}
```

- [x] **Step 2: 运行测试确认失败**

Run:

```bash
cd src-tauri && cargo test --test python_run_scope_test python_runner_uses_managed_python_from_runtime_resolver -- --nocapture
```

Expected: FAIL because PythonRunner does not expose resolver constructor/test getter.

- [x] **Step 3: 修改 PythonRunner 构造与执行路径，保留兼容入口**

Modify `src-tauri/src/python/runner.rs` so `PythonRunner` can be constructed from a resolver without deleting the existing `with_runtime(...)` / `with_config_from_path(...)` compatibility entry points.

```rust
use std::sync::Arc;

use crate::runtime::dependencies::RuntimeResolver;

impl PythonRunner {
    pub fn with_runtime_resolver(
        workspace_path: PathBuf,
        sandbox: SandboxConfig,
        runtime_resolver: Arc<dyn RuntimeResolver>,
    ) -> Result<Self> {
        let deps = runtime_resolver.workspace_dependencies().map_err(|err| anyhow!(err.to_string()))?;
        Ok(Self::with_runtime(workspace_path, sandbox, deps.python, None))
    }

    #[cfg(test)]
    pub fn for_tests_with_runtime_resolver(runtime_resolver: Arc<dyn RuntimeResolver>) -> Self {
        Self::with_runtime_resolver(
            PathBuf::from("/tmp/renlijia/workspace"),
            SandboxConfig::for_workspace(&PathBuf::from("/tmp/renlijia/workspace")),
            runtime_resolver,
        )
        .expect("test runtime resolver should resolve")
    }

    #[cfg(test)]
    pub fn python_executable_for_tests(&self) -> std::path::PathBuf {
        self.python_binary.clone()
    }
}
```

Do not remove `resolve_python_path(...)` in this task. It remains as the legacy fallback until Task 4.3 migrates all construction sites.

- [x] **Step 4: 运行 PythonRunner 相关测试**

Run:

```bash
cd src-tauri && cargo test --test python_run_scope_test -- --nocapture
```

Expected: PASS.

- [x] **Step 5: 提交 PythonRunner 接入**

```bash
git add src-tauri/src/python/runner.rs src-tauri/tests/python_run_scope_test.rs
git commit -m "feat: run python through managed runtime resolver"
```

### Task 4.3: 迁移现有 Python runtime 注入链路

**Files:**
- Modify: `src-tauri/src/plugin/registry.rs`
- Modify: `src-tauri/src/llm/tool_executor/file_load.rs`
- Modify: `src-tauri/src/runtime/agent/worker_runtime.rs`
- Modify: `src-tauri/src/runtime/tools/builtin/python.rs`
- Modify: `src-tauri/src/runtime/tools/builtin/python_execution.rs`
- Modify: `src-tauri/src/runtime/tools/builtin/chart_capability.rs`
- Modify: `src-tauri/src/runtime/tools/builtin/report_capability.rs`
- Test: `src-tauri/tests/runtime_dependencies_python_injection_test.rs`

- [x] **Step 1: 写失败测试，确认注册链路可以注入托管 Python**

Create `src-tauri/tests/runtime_dependencies_python_injection_test.rs`:

```rust
use std::path::PathBuf;
use std::sync::Arc;

use app_lib::runtime::dependencies::{RuntimeResolver, StaticRuntimeResolver};
use app_lib::runtime::tools::builtin::python::ExecutePythonRuntimeTool;

#[test]
fn execute_python_tool_can_be_constructed_from_managed_runtime_dependencies() {
    let resolver = Arc::new(StaticRuntimeResolver::new(
        PathBuf::from("/tmp/renlijia/python/bin/python3"),
        PathBuf::from("/tmp/renlijia/node/bin/node"),
        PathBuf::from("/tmp/renlijia/node/bin/npm"),
        PathBuf::from("/tmp/renlijia/node/bin/npx"),
        PathBuf::from("/tmp/renlijia/uv"),
        PathBuf::from("/tmp/renlijia/uvx"),
        PathBuf::from("/tmp/renlijia/node/node_modules"),
        PathBuf::from("/tmp/renlijia/python/site-packages"),
    ));
    let deps = resolver.workspace_dependencies().expect("deps should resolve");

    let tool = ExecutePythonRuntimeTool::with_runtime_deps(
        PathBuf::from("/tmp/renlijia/workspace"),
        deps.python.clone(),
        None,
    );

    assert_eq!(tool.python_binary_path(), Some(&deps.python));
}
```

- [x] **Step 2: 运行测试确认当前构造路径可被测试覆盖**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_python_injection_test -- --nocapture
```

Expected: PASS if existing `with_runtime_deps` is already public; otherwise FAIL and expose the exact visibility gap to fix in Step 3.

- [x] **Step 3: 将生产构造点改为优先使用 RuntimeResolver**

For each call site currently using `crate::python::runner::resolve_python_path(...)`, replace the production path with this pattern:

```rust
let deps = runtime_resolver.workspace_dependencies().map_err(|err| anyhow::anyhow!(err.to_string()))?;
let python_binary = deps.python;
let python_home = None;
```

Apply the pattern to these searched locations:

```bash
rg -n "resolve_python_path|python_binary: Some|with_runtime_deps|with_runtime\(" src-tauri/src/plugin/registry.rs src-tauri/src/llm/tool_executor/file_load.rs src-tauri/src/runtime/agent/worker_runtime.rs src-tauri/src/runtime/tools/builtin
```

Keep `resolve_python_path(...)` only as a compatibility fallback for call sites that have no resolver yet, and add a comment at the fallback boundary explaining why it still exists.

- [x] **Step 4: 运行 Python 注入链路测试和关键回归**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_python_injection_test --test python_run_scope_test --test builtin_runtime_registration_test --no-fail-fast
```

Expected: all selected tests PASS.

- [x] **Step 5: 提交 Python 注入链路迁移**

```bash
git add src-tauri/src/plugin/registry.rs src-tauri/src/llm/tool_executor/file_load.rs src-tauri/src/runtime/agent/worker_runtime.rs src-tauri/src/runtime/tools/builtin src-tauri/tests/runtime_dependencies_python_injection_test.rs
git commit -m "feat: inject managed python runtime through tool construction"
```

---

## Phase 5：MCP 与命令占位符

### Task 5.1: MCP stdio command 支持 `${renlijia.*}` 占位符

**Files:**
- Create: `src-tauri/src/runtime/mcp/command_template.rs`
- Modify: `src-tauri/src/runtime/mcp/connection.rs`
- Modify if needed: `src-tauri/src/runtime/mcp/manager.rs`
- Test: `src-tauri/tests/runtime_dependencies_mcp_placeholder_test.rs`

- [x] **Step 1: 写失败测试**

Create `src-tauri/tests/runtime_dependencies_mcp_placeholder_test.rs`:

```rust
use std::path::PathBuf;
use std::sync::Arc;

use app_lib::runtime::dependencies::StaticRuntimeResolver;
use app_lib::runtime::mcp::command_template::resolve_mcp_command_template;

#[test]
fn mcp_command_template_resolves_renlijia_node_placeholder() {
    let resolver = Arc::new(StaticRuntimeResolver::new(
        PathBuf::from("/tmp/renlijia/python/bin/python3"),
        PathBuf::from("/tmp/renlijia/node/bin/node"),
        PathBuf::from("/tmp/renlijia/node/bin/npm"),
        PathBuf::from("/tmp/renlijia/node/bin/npx"),
        PathBuf::from("/tmp/renlijia/uv"),
        PathBuf::from("/tmp/renlijia/uvx"),
        PathBuf::from("/tmp/renlijia/node/node_modules"),
        PathBuf::from("/tmp/renlijia/python/site-packages"),
    ));

    let resolved = resolve_mcp_command_template("${renlijia.node}", resolver.as_ref())
        .expect("template should resolve");

    assert_eq!(resolved, PathBuf::from("/tmp/renlijia/node/bin/node"));
}
```

- [x] **Step 2: 运行测试确认失败**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_mcp_placeholder_test -- --nocapture
```

Expected: FAIL because `command_template` does not exist.

- [x] **Step 3: 实现 command template**

Create `src-tauri/src/runtime/mcp/command_template.rs`:

```rust
use std::path::PathBuf;

use crate::runtime::dependencies::{RuntimeResolver, RuntimeDependencyError};

pub fn resolve_mcp_command_template(
    command: &str,
    resolver: &dyn RuntimeResolver,
) -> Result<PathBuf, RuntimeDependencyError> {
    let deps = resolver.workspace_dependencies()?;
    let path = match command {
        "${renlijia.node}" => deps.node,
        "${renlijia.npm}" => deps.npm,
        "${renlijia.npx}" => deps.npx,
        "${renlijia.python}" => deps.python,
        "${renlijia.uv}" => deps.uv,
        "${renlijia.uvx}" => deps.uvx,
        other => PathBuf::from(other),
    };
    Ok(path)
}
```

Modify `src-tauri/src/runtime/mcp/mod.rs`:

```rust
pub mod command_template;
```

Modify `src-tauri/src/runtime/mcp/connection.rs` so `StdioMcpConnection::connect()` resolves the parsed program with `resolve_mcp_command_template` before `Command::new(...)`. 如果直接把 resolver 存到 `StdioMcpConnection` 会造成大范围改动，则新增一个小的 `StdioCommandResolver` 抽象，并在单元测试中注入静态测试 resolver。只有当 connection 构造必须传递 resolver 时才Modify `manager.rs`；它不能拥有 spawn 逻辑。

- [x] **Step 4: 运行 MCP placeholder 测试**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_mcp_placeholder_test -- --nocapture
```

Expected: PASS.

- [x] **Step 5: 提交 MCP 占位符支持**

```bash
git add src-tauri/src/runtime/mcp src-tauri/tests/runtime_dependencies_mcp_placeholder_test.rs
git commit -m "feat: resolve managed runtime placeholders for mcp commands"
```

---

## Phase 6：前端健康状态与用户操作

### Task 6.1: 后端 Runtime Tauri commands

**Files:**
- Create: `src-tauri/src/transport/tauri_commands/runtime.rs`
- Modify: `src-tauri/src/transport/tauri_commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/tests/runtime_commands_test.rs`

- [x] **Step 1: 写失败测试，验证 command payload 可以序列化**

Create `src-tauri/tests/runtime_commands_test.rs`:

```rust
use app_lib::transport::tauri_commands::runtime::{RuntimeHealthPayload, RuntimeToolHealthPayload};

#[test]
fn runtime_health_payload_serializes_with_camel_case_fields() {
    let payload = RuntimeHealthPayload {
        bundle_version: "2026.04.25".to_string(),
        node: Some(RuntimeToolHealthPayload {
            version: "v22.19.0".to_string(),
            path: "/tmp/renlijia/node/bin/node".to_string(),
        }),
        python: None,
        uv: None,
    };

    let json = serde_json::to_value(payload).expect("serialize payload");

    assert_eq!(json["bundleVersion"], "2026.04.25");
    assert_eq!(json["node"]["version"], "v22.19.0");
}
```

- [x] **Step 2: 运行测试确认失败**

Run:

```bash
cd src-tauri && cargo test --test runtime_commands_test -- --nocapture
```

Expected: FAIL because runtime command module does not exist.

- [x] **Step 3: 实现 runtime command payload 和 command 函数**

Create `src-tauri/src/transport/tauri_commands/runtime.rs`:

```rust
use serde::Serialize;
use tauri::State;

use crate::runtime::dependencies::RuntimeResolver;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeToolHealthPayload {
    pub version: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeHealthPayload {
    pub bundle_version: String,
    pub node: Option<RuntimeToolHealthPayload>,
    pub python: Option<RuntimeToolHealthPayload>,
    pub uv: Option<RuntimeToolHealthPayload>,
}

#[tauri::command]
pub async fn runtime_get_health(
    resolver: State<'_, crate::runtime::dependencies::ManagedRuntimeResolver>,
) -> Result<RuntimeHealthPayload, String> {
    let deps = resolver.workspace_dependencies().map_err(|err| err.to_string())?;
    Ok(RuntimeHealthPayload {
        bundle_version: "2026.04.25".to_string(),
        node: Some(RuntimeToolHealthPayload {
            version: "managed".to_string(),
            path: deps.node.display().to_string(),
        }),
        python: Some(RuntimeToolHealthPayload {
            version: "managed".to_string(),
            path: deps.python.display().to_string(),
        }),
        uv: Some(RuntimeToolHealthPayload {
            version: "managed".to_string(),
            path: deps.uv.display().to_string(),
        }),
    })
}

#[tauri::command]
pub async fn runtime_ensure(
    resolver: State<'_, crate::runtime::dependencies::ManagedRuntimeResolver>,
) -> Result<RuntimeHealthPayload, String> {
    runtime_get_health(resolver).await
}

#[tauri::command]
pub async fn runtime_reinstall(
    resolver: State<'_, crate::runtime::dependencies::ManagedRuntimeResolver>,
) -> Result<RuntimeHealthPayload, String> {
    runtime_get_health(resolver).await
}
```

Modify `src-tauri/src/transport/tauri_commands/mod.rs`:

```rust
pub mod runtime;
```

Modify `src-tauri/src/lib.rs` invoke handler list by adding:

```rust
transport::tauri_commands::runtime::runtime_get_health,
transport::tauri_commands::runtime::runtime_ensure,
transport::tauri_commands::runtime::runtime_reinstall,
```

- [x] **Step 4: 运行后端 command 测试**

Run:

```bash
cd src-tauri && cargo test --test runtime_commands_test -- --nocapture
```

Expected: PASS.

- [x] **Step 5: 提交后端 commands**

```bash
git add src-tauri/src/transport/tauri_commands src-tauri/src/lib.rs src-tauri/tests/runtime_commands_test.rs
git commit -m "feat: expose runtime health tauri commands"
```

### Task 6.2: Tauri runtime API wrapper

**Files:**
- Modify: `src/lib/tauri.ts`
- Create: `src/lib/tauri.runtime.test.ts`

- [x] **Step 1: 写失败测试**

Create `src/lib/tauri.runtime.test.ts`:

```ts
import { describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

import { invoke } from '@tauri-apps/api/core';
import { getRuntimeHealth } from './tauri';

describe('runtime tauri api', () => {
  it('invokes backend runtime health command', async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      bundleVersion: '2026.04.25',
      node: { version: 'v22.19.0', path: '/tmp/node' },
      python: { version: 'Python 3.12.13', path: '/tmp/python' },
      uv: { version: 'uv 0.7.13', path: '/tmp/uv' },
    });

    const result = await getRuntimeHealth();

    expect(invoke).toHaveBeenCalledWith('runtime_get_health');
    expect(result.bundleVersion).toBe('2026.04.25');
  });
});
```

- [x] **Step 2: 运行测试确认失败**

Run:

```bash
pnpm exec vitest run src/lib/tauri.runtime.test.ts
```

Expected: FAIL because `getRuntimeHealth` does not exist.

- [x] **Step 3: 增加 wrapper**

Modify `src/lib/tauri.ts`:

```ts
export type RuntimeToolHealth = {
  version: string;
  path: string;
};

export type RuntimeHealth = {
  bundleVersion: string;
  node: RuntimeToolHealth | null;
  python: RuntimeToolHealth | null;
  uv: RuntimeToolHealth | null;
};

export async function getRuntimeHealth(): Promise<RuntimeHealth> {
  return invoke<RuntimeHealth>('runtime_get_health');
}

export async function ensureRuntime(): Promise<RuntimeHealth> {
  return invoke<RuntimeHealth>('runtime_ensure');
}

export async function reinstallRuntime(): Promise<RuntimeHealth> {
  return invoke<RuntimeHealth>('runtime_reinstall');
}
```

Keep existing imports and exports intact.

- [x] **Step 4: 运行 wrapper 测试**

Run:

```bash
pnpm exec vitest run src/lib/tauri.runtime.test.ts
```

Expected: PASS.

- [x] **Step 5: 提交前端 API wrapper**

```bash
git add src/lib/tauri.ts src/lib/tauri.runtime.test.ts
git commit -m "feat: add frontend runtime tauri api"
```

### Task 6.3: Runtime store

**Files:**
- Create: `src/stores/runtimeStore.ts`
- Create: `src/stores/runtimeStore.test.ts`

- [x] **Step 1: 写失败测试**

Create `src/stores/runtimeStore.test.ts`:

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../lib/tauri', () => ({
  getRuntimeHealth: vi.fn(),
}));

import { getRuntimeHealth } from '../lib/tauri';
import { useRuntimeStore } from './runtimeStore';

describe('runtimeStore', () => {
  beforeEach(() => {
    useRuntimeStore.setState({ status: 'idle', health: null, error: null });
    vi.clearAllMocks();
  });

  it('loads runtime health into state', async () => {
    vi.mocked(getRuntimeHealth).mockResolvedValueOnce({
      bundleVersion: '2026.04.25',
      node: { version: 'v22.19.0', path: '/tmp/node' },
      python: { version: 'Python 3.12.13', path: '/tmp/python' },
      uv: { version: 'uv 0.7.13', path: '/tmp/uv' },
    });

    await useRuntimeStore.getState().refresh();

    expect(useRuntimeStore.getState().status).toBe('ready');
    expect(useRuntimeStore.getState().health?.node?.version).toBe('v22.19.0');
  });
});
```

- [x] **Step 2: 运行测试确认失败**

Run:

```bash
pnpm exec vitest run src/stores/runtimeStore.test.ts
```

Expected: FAIL because store does not exist.

- [x] **Step 3: 实现 runtime store**

Create `src/stores/runtimeStore.ts`:

```ts
import { create } from 'zustand';
import { getRuntimeHealth, type RuntimeHealth } from '../lib/tauri';

type RuntimeStatus = 'idle' | 'loading' | 'ready' | 'error';

type RuntimeStore = {
  status: RuntimeStatus;
  health: RuntimeHealth | null;
  error: string | null;
  refresh: () => Promise<void>;
};

export const useRuntimeStore = create<RuntimeStore>((set) => ({
  status: 'idle',
  health: null,
  error: null,
  refresh: async () => {
    set({ status: 'loading', error: null });
    try {
      const health = await getRuntimeHealth();
      set({ status: 'ready', health, error: null });
    } catch (error) {
      set({
        status: 'error',
        health: null,
        error: error instanceof Error ? error.message : String(error),
      });
    }
  },
}));
```

- [x] **Step 4: 运行 store 测试**

Run:

```bash
pnpm exec vitest run src/stores/runtimeStore.test.ts
```

Expected: PASS.

- [x] **Step 5: 提交 runtime store**

```bash
git add src/stores/runtimeStore.ts src/stores/runtimeStore.test.ts
git commit -m "feat: add runtime status store"
```

### Task 6.4: 移除 legacy `src-tauri/python-runtime` 打包路径

**Files:**
- Modify: `src-tauri/tauri.conf.json`
- Modify: `scripts/setup-python.sh`
- Modify: `scripts/setup-python.ps1`
- Modify: `.github/workflows/build-desktop.yml`
- Modify: `.github/workflows/ci.yml`
- Test: `src-tauri/tests/runtime_dependencies_no_legacy_resource_test.rs`

- [x] **Step 1: 写失败测试，确认 Tauri resources 不再声明 python-runtime**

Create `src-tauri/tests/runtime_dependencies_no_legacy_resource_test.rs`:

```rust
use std::fs;

#[test]
fn tauri_config_does_not_package_legacy_python_runtime_resource() {
    let config = fs::read_to_string("tauri.conf.json").expect("read tauri config");

    assert!(
        !config.contains("python-runtime"),
        "python-runtime is legacy and must not be packaged as an app resource"
    );
}
```

- [x] **Step 2: 运行测试确认失败**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_no_legacy_resource_test -- --nocapture
```

Expected: FAIL because `tauri.conf.json` still maps `src/python` to `python-runtime`.

- [x] **Step 3: 移除 Tauri resource 里的 legacy Python runtime**

Modify `src-tauri/tauri.conf.json` resources from:

```json
"resources": {
  "src/python": "python-runtime",
  "playwright-runtime": "playwright-runtime",
  "prompts": "prompts",
  "plugins": "plugins"
}
```

To:

```json
"resources": {
  "playwright-runtime": "playwright-runtime",
  "prompts": "prompts",
  "plugins": "plugins"
}
```

- [x] **Step 4: 停用旧 setup/cache 路径**

Modify `scripts/setup-python.sh` and `scripts/setup-python.ps1` so they no longer write to `src-tauri/python-runtime`. Remove these scripts from the production build path. Runtime artifacts are produced by `scripts/build-runtime-bundle.py` and downloaded by the app through manifest-driven RuntimeInstaller.

Modify `.github/workflows/build-desktop.yml` and `.github/workflows/ci.yml` to remove cache/bootstrap steps for `src-tauri/python-runtime`.

- [x] **Step 5: 验证 legacy 路径已从生产配置移除**

Run:

```bash
rg -n "src-tauri/python-runtime|src/python.:.python-runtime|python-runtime" src-tauri/tauri.conf.json scripts .github/workflows
cd src-tauri && cargo test --test runtime_dependencies_no_legacy_resource_test -- --nocapture
```

Expected: the `rg` command has no production config hits for packaging `python-runtime`; the test PASS.

- [x] **Step 6: 提交 legacy runtime 移除**

```bash
git add src-tauri/tauri.conf.json scripts/setup-python.sh scripts/setup-python.ps1 .github/workflows src-tauri/tests/runtime_dependencies_no_legacy_resource_test.rs
git commit -m "chore: remove legacy bundled python runtime resource"
```

---

## Phase 7：最终验证与收敛

### Task 7.1: 后端窄范围验证

**Files:**
- No file changes expected.

- [x] **Step 1: 运行 runtime dependencies 测试组合**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_platform_test --test runtime_dependencies_paths_test --test runtime_dependencies_manifest_test --test runtime_dependencies_resolver_test --test runtime_dependencies_archive_test --test runtime_dependencies_health_test --test runtime_dependencies_mcp_placeholder_test --no-fail-fast
```

Expected: all selected tests PASS.

- [x] **Step 2: 运行 Python/MCP 相关回归**

Run:

```bash
cd src-tauri && cargo test --test python_run_scope_test --test mcp_registry_integration_test --test mcp_types_and_trait_test --no-fail-fast
```

Expected: all selected tests PASS.

- [x] **Step 3: 运行 cargo check**

Run:

```bash
cd src-tauri && cargo check
```

Expected: command exits 0.

### Task 7.2: 前端验证

**Files:**
- No file changes expected.

- [x] **Step 1: 运行 runtime 前端测试**

Run:

```bash
pnpm exec vitest run src/lib/tauri.runtime.test.ts src/stores/runtimeStore.test.ts
```

Expected: all tests PASS.

- [x] **Step 2: 运行 TypeScript build**

Run:

```bash
pnpm build
```

Expected: command exits 0.

### Task 7.3: 文档与计划自查

**Files:**
- Modify if needed: `docs/runtime-manager.md`
- Modify if needed: `docs/superpowers/plans/2026-04-25-runtime-manager.md`

- [x] **Step 1: 搜索占位符**

Run:

```bash
rg -n "占位符|未定义实现|临时方案" docs/runtime-manager.md docs/superpowers/plans/2026-04-25-runtime-manager.md
```

Expected: no output.

- [x] **Step 2: 检查代码没有硬编码新 runtime 路径散落**

Run:

```bash
rg -n "renlijia-runtimes|src-tauri/python-runtime|python-runtime/bin/python|\.cache/codex-runtimes|\.real/\.bin" src-tauri/src
```

Expected: only migration docs or explicit legacy-removal tests mention `python-runtime`; runtime/tool business code must not construct paths from `src-tauri/python-runtime`.

- [x] **Step 3: 最终提交**

```bash
git status --short
git add docs/runtime-manager.md docs/superpowers/plans/2026-04-25-runtime-manager.md src-tauri src
git commit -m "feat: add managed runtime resolver plan implementation"
```

---

## 完整方案后续专项

这些能力属于完整 runtime 体系的一部分；如果本计划执行周期过长，可以拆成后续专项，但不能作为临时方案省略：

1. 远程 manifest 拉取和签名验证。
2. 代理配置、企业镜像源、禁用自动下载。
3. 离线 runtime 包导入。
4. Runtime 版本并存、current symlink/指针切换和自动回滚。
5. UI 下载进度、取消下载、重试、清理旧版本。
6. Workspace `.venv` / `node_modules` 自动隔离与依赖安装权限确认。
7. 针对用户代码执行的更强沙箱：网络权限、资源限制、进程树 kill、输出限流统一封装。

---

## Self-Review

- Spec coverage: 本计划覆盖私有 runtime 目录、Resolver、manifest、checksum、安全解压、health、installer 骨架、PythonRunner 接入、MCP 占位符、前端 health API/store、验证命令。
- Placeholder scan: 文档没有保留未定义实现项；生产 manifest、下载源、企业镜像和旧 runtime 移除都已纳入完整方案边界。
- Type consistency: `WorkspaceDependencies`、`RuntimeResolver`、`StaticRuntimeResolver`、`RuntimePaths`、`RuntimeManifest`、`RuntimeHealthChecker` 在前置任务定义，后续任务按同名接口使用。
- Scope check: 本计划仍然偏大，建议执行时按 Phase 串行推进；如果要并行，只能拆分为互不冲突写集：文档、manifest/paths、前端 store/UI、MCP placeholder。`PythonRunner` 与 `CapabilityContext` 必须串行。

### 当前实现补充：artifact 安装边界

当前代码已经补齐下载完成后的安装边界：`RuntimeInstaller::install_from_verified_archive(...)` 会先校验 sha256，再安全解压 zip 到 staging，校验 runtime payload，最后原子切换 `versions/<version>` 与 `current` 指针。`RuntimeManager::install_verified_archive(...)` 暴露同一能力，供后续 manifest/downloader 把公网 artifact 下载到本地后接入。

当前代码也已经提供公网下载 orchestrator 入口：`RuntimeManager::install_from_manifest_url(...)` 会读取 HTTPS manifest、选择当前平台 artifact、下载 HTTPS artifact 到 `downloads/`，然后调用 verified archive 安装。普通单元测试不真实访问公网，只覆盖本地 file manifest/file artifact 与 untrusted URL 拒绝；真实公网下载仍应放在 ignored/nightly 或手动 smoke test。
