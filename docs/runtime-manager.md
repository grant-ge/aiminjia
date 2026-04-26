# Lotus / Renlijia Runtime Manager 架构说明

## 1. 背景

Lotus 的核心能力不是只提供一个桌面界面，而是在用户本机稳定地执行脚本、工具、MCP server 和自动化任务。只要产品要支持文件分析、图表生成、表格处理、Python 执行、Node 脚本、依赖安装或本地服务启动，就一定会碰到运行环境管理问题。

如果这些能力依赖用户自己安装 Python、Node、uv、npm，产品就会变成不稳定的：同一个功能在不同机器上可能因为 PATH、版本、权限或包管理器差异而失败。Runtime Manager 的目的，就是把“本机能不能执行代码”从不可控的用户环境问题，变成 Lotus 自己可交付、可校验、可恢复的基础设施能力。

## 2. 为什么需要 Runtime Manager

Runtime Manager 需要回答的不是“有没有一个 Python 目录”，而是下面这些基础问题：

- 这次执行实际用了哪个 Python、Node、uv？
- 这些 runtime 是否由 Lotus / Renlijia 下载、校验并安装过？
- 用户机器没有预装相关工具时，产品是否仍然可以工作？
- runtime 损坏、升级失败、版本不兼容时，是否可以自动恢复或回滚？
- App 升级和 runtime 升级是否可以解耦？
- Python 工具、Node 工具、MCP 执行是否共享同一套 runtime 边界？

如果这些问题没有统一答案，后续任何新的本地执行能力都会继续把环境不确定性扩散到更多模块里。

## 3. 目标

Runtime Manager 的目标是形成一套可长期维护的本地执行基础设施：

- Node、Python、uv 由 Lotus / Renlijia 托管，不要求用户预装。
- 工具执行必须使用 Runtime Manager 返回的绝对路径。
- Runtime 和 App 本体解耦，可以独立升级、重装和回滚。
- 下载包必须由 manifest 指定并通过 sha256 校验。
- 用户 workspace 依赖与 Renlijia 内置工具依赖隔离。
- 企业环境支持镜像源、禁用自动下载和离线导入。
- 后端、前端和设置页都通过同一套 health / ensure / reinstall 能力观察 runtime 状态。

## 4. 为什么 `src-tauri/python-runtime` 是 legacy

`src-tauri/python-runtime` 只能视为迁移期遗留方案，不是目标架构。

它的问题很明确：

- **和 App 构建强耦合**：Python 升级、依赖修复、runtime 损坏恢复都会变成 App 发版问题。
- **目录语义不对**：`src-tauri` 是源码和构建目录，不应该长期承载生产 runtime。
- **能力覆盖太窄**：旧方案只能解释 Python，无法统一 Node、uv、npm/npx、MCP server 和后续插件运行时。
- **容易散落路径拼接**：业务代码一旦知道 `python-runtime/bin/python3`，runtime 能力就会被写死到各处，后面很难统一权限、升级、健康检查和回滚。

因此，`src-tauri/python-runtime` 只能作为迁移期兼容入口，不能作为新功能和长期运行链路的依赖来源。

## 5. 目标目录

Runtime Manager 的目标存放位置是用户缓存目录，而不是源码目录或用户数据目录：

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

Windows 默认目录：

```text
%LOCALAPPDATA%\Renlijia\runtimes\
  renlijia-primary-runtime\
    current
    versions\
      2026.04.25\
        install.json
        dependencies\
          node\
          python\
          uv
          uvx
    downloads\
    staging\
```

这套目录结构的含义如下：

- `versions/`：支持多版本并存，便于升级、回滚和排查。
- `staging/`：支持先下载、解压、校验，再原子切换。
- `current`：表示当前启用版本，便于健康检查和恢复。
- `downloads/`：缓存下载产物，支持重复安装复用和断点恢复。

这个目录只存可重建的 runtime 制品，不存 workspace、会话、设置或用户业务数据。删除整个 `renlijia-runtimes` 目录，只会触发 runtime 重新下载，不会影响用户数据本身。

## 6. 执行规则

Runtime Manager 的执行规则不是“启动时一次性装完”，而是“预检查 + 首次强保证 + 可恢复切换”。

业务代码禁止直接拼 runtime 路径，必须通过 `RuntimeResolver` 获取下列能力：

```text
node
npm
npx
python
uv
uvx
node_modules
python_site_packages
```

工具层只能使用这些绝对路径执行命令，不能退回系统 PATH，也不能读取 `src-tauri/python-runtime`。

### 6.1 启动后的后台预检查

App 启动后可以创建 `RuntimeResolver`，后台检查 `current` 是否存在、版本是否匹配、健康检查是否通过。这个阶段允许预热，但不应该阻塞主界面。

### 6.2 首次使用前强制 ensure

当用户第一次触发任何需要本地 runtime 的能力时，执行链路必须先调用 `RuntimeResolver.workspace_dependencies()`，再由它触发 `RuntimeInstaller.ensure(...)`。

典型触发点包括：

- `execute_python`
- Node / script 执行
- 图表、报表、表格、文件处理等需要本地依赖的功能
- MCP stdio 或本地工具执行

如果 runtime 缺失，就必须在这里完成下载、校验、解压和 smoke test，然后才允许后续执行继续。

### 6.3 用户主动修复

设置页应提供运行环境维护入口，例如：

- 检查运行环境
- 重新安装运行环境
- 清理旧版本

这些操作都应当走同一套 ensure / reinstall / cleanup 逻辑，而不是单独写一条特殊修复链路。

### 6.4 升级与切换

runtime manifest 允许更新，但不能在新版本未验证前破坏当前可用版本。正确流程是：

1. 下载新 manifest
2. 下载对应 artifact
3. 校验 sha256
4. 解压到 staging
5. 做 smoke test
6. 成功后再切换 `current`

任何一步失败，都不能覆盖现有可用版本。

## 7. 包管理边界

Renlijia 不做通用包管理，也不尝试自研 npm / PyPI / lockfile 解析器。Runtime Manager 的职责只包括基础 runtime bundle 的交付和管理。

### 7.1 Runtime artifact 管理

- 下载 runtime bundle
- 校验 sha256
- 解压到 staging
- 执行 smoke test
- 切换 `current`
- 回滚到可用版本
- 暴露 node / python / uv 的绝对路径

### 7.2 第三方生态工具负责

- Node 包依赖解析、安装和 lockfile 语义
- Python 包依赖解析、安装和虚拟环境语义
- workspace 依赖解析与同步
- lockfile 和 package metadata 处理
- 第三方包的安全公告和撤回策略

### 7.3 默认内置的工具边界

Runtime bundle 只需要带上产品执行所需的基础工具，不引入额外总包管理器：

```text
Node 侧:
  node
  npm
  npx
  corepack

Python 侧:
  python3
  pip
  uv
  uvx
```

Node workspace 仍然遵循 Node 生态自身规则，Python workspace 仍然遵循 Python 生态自身规则：

- `package.json` / `package-lock.json` / `pnpm-lock.yaml` / `yarn.lock` 由 npm / npx / corepack 处理
- `pyproject.toml` / `uv.lock` / `requirements.txt` 由 uv / uvx / pip 处理

Renlijia 负责把这些基础工具稳定带到用户机器上，但不替代它们的包管理语义。

## 8. 下载与安装流程

### 8.1 版本来源

下载源应分成三层，不把单一 URL 写死在业务代码中：

1. **官方 Renlijia runtime manifest**：生产默认从受控 manifest 获取版本、下载地址和 sha256。
2. **受控 runtime artifact**：生产推荐使用 Renlijia CI 预下载、校验并重新打包后的 bundle，而不是直接依赖外部下载地址。
3. **企业或离线镜像**：通过环境变量或设置覆盖 manifest base URL，满足内网和镜像部署需求。

### 8.2 推荐的生产路径

推荐由 Renlijia CI 统一构建 runtime bundle，而不是让用户 App 直接拉多个上游包作为生产默认。原因是 Renlijia bundle 可以统一目录结构、减少客户端下载次数、预置内置依赖、稳定 sha256 和版本语义，也更适合企业镜像、离线导入和回滚。

推荐由 Renlijia CI 统一构建 runtime bundle：

```text
Renlijia CI
  -> 下载 Node 官方包 / python-build-standalone / uv
  -> 组织为 renlijia-primary-runtime 目录结构
  -> 生成 tar.gz / zip
  -> 计算 sha256
  -> 生成 manifest.json
  -> 上传到 OSS / CDN

用户 App
  -> 下载 manifest.json
  -> 下载对应平台 artifact
  -> 校验 sha256
  -> 解压到 staging
  -> smoke test
  -> 切换 current
```

### 8.3 安装原则

- 下载前先确认平台和版本是否匹配。
- 下载后必须做 sha256 校验。
- 校验通过后才允许解压。
- 解压到 staging，不能直接覆盖当前可用版本。
- smoke test 通过后再切换 `current`。
- 任一步失败都保留旧版本，避免把当前可用 runtime 搞坏。

## 9. 验证策略

Runtime Manager 的验证目标，不是“文件存在”，而是“可执行、可恢复、可切换”。

### 9.1 安装验证

安装完成后至少要验证：

- 目录结构完整
- 版本信息可读
- runtime 二进制存在且可执行
- `python3` / `node` / `uv` 等基础命令可以启动
- smoke test 返回成功

### 9.2 健康检查

启动时后台健康检查应关注：

- `current` 是否存在
- `install.json` 是否可解析
- 当前版本是否与 manifest 兼容
- 基础二进制是否仍可执行

### 9.3 故障恢复

恢复策略应优先选择最保守的方式：

- `current` 损坏时，尝试回滚到已知可用版本
- 下载或校验失败时，不覆盖现有版本
- smoke test 失败时，不切换版本
- manifest 异常时，保留上一次成功安装的 runtime

### 9.4 开发验证

开发环境可以使用本地测试 manifest 或本地 artifact，但只作为验证手段，不作为生产设计。生产路径必须以受控 manifest 和校验过的 artifact 为准。

## 10. 非目标与结论

Runtime Manager 的定位是 Lotus / Renlijia 的本机执行基础设施，不是某个 Python 目录的搬家工程。

最终边界可以概括为：

- `src-tauri/python-runtime` 是 legacy，只保留迁移兼容价值。
- 目标目录放在 `~/.cache/renlijia-runtimes/`，作为可重建缓存。
- Runtime Manager 只交付和管理基础 runtime bundle，不做通用包管理。
- 下载、安装、校验、切换、回滚都必须可验证、可恢复、可重复。
- Python、Node、uv、MCP 等能力在这个基础之上各自遵守自己的生态规则。

这就是 Lotus Runtime Manager 的长期架构边界。

### 当前实现补充：artifact 安装边界

当前代码已经补齐下载完成后的安装边界：`RuntimeInstaller::install_from_verified_archive(...)` 会先校验 sha256，再安全解压 zip 到 staging，校验 runtime payload，最后原子切换 `versions/<version>` 与 `current` 指针。`RuntimeManager::install_verified_archive(...)` 暴露同一能力，供后续 manifest/downloader 把公网 artifact 下载到本地后接入。

当前代码也已经提供公网下载 orchestrator 入口：`RuntimeManager::install_from_manifest_url(...)` 会读取 HTTPS manifest、选择当前平台 artifact、下载 HTTPS artifact 到 `downloads/`，然后调用 verified archive 安装。普通单元测试不真实访问公网，只覆盖本地 file manifest/file artifact 与 untrusted URL 拒绝；真实公网下载仍应放在 ignored/nightly 或手动 smoke test。

## 当前落地的下载触发流程

仁励家不自己实现 Node/Python 的上游包管理器，也不在用户机器上临时运行安装脚本去拼装环境。发布侧提前产出一个完整 runtime artifact，应用侧只负责可信下载、校验、解压、smoke test 和版本切换。

生产默认内置 Renlijia 受控 manifest：`https://datamind-pzc.oss-cn-hangzhou.aliyuncs.com/runtimes/runtime-manifest.json`。`RENLIJIA_RUNTIME_MANIFEST_URL` 只作为企业镜像、离线部署或开发测试的覆盖入口，不是普通用户必须配置的环境变量。manifest 指向 `renlijia-primary-runtime` 的平台 artifact，artifact 内已经包含：

- `node/bin/node`、`node/bin/npm`、`node/bin/npx`
- `python/bin/python3`
- `uv/bin/uv`、`uv/bin/uvx`
- `node/node_modules/`
- `python/lib/site-packages/`
- `install.json` 由安装器写入当前 bundle version

下载触发点分三层：

1. **启动后台预检查（默认启用）**：Tauri setup 使用内置 manifest 创建 `RuntimeManager`，随后后台调用 `RuntimeManager::ensure_managed()` 预热 runtime。这个任务不阻塞主界面；失败只记录 warning，首次工具执行前仍会强制 ensure。
2. **显式用户操作**：设置页/前端调用 `runtime_ensure` 或 `runtime_reinstall` 时，后端 command 通过 `RuntimeManager::ensure_managed()` / `reinstall_managed()` 使用 manifest 下载并安装。
3. **首次本地执行前强保证**：业务工具、Python 工具、MCP stdio 注入的 resolver 是 `RuntimeManager`。当 `workspace_dependencies()` 发现 `current` 不存在或损坏，且已配置 manifest，它会先 ensure，再返回 Node/Python/uv 路径；因此工具不会静默退回系统 Python/Node。

安装顺序固定为：

```text
读取 manifest
  -> 选择当前平台 artifact
  -> 下载到 downloads/
  -> 校验 sha256
  -> 解压到 staging/<version>
  -> 校验必需文件
  -> 在 staging 执行 node/npm/npx/python/uv/uvx --version smoke test
  -> 移动到 versions/<version>
  -> 原子写 current pointer
```

失败策略：manifest 拉取失败、checksum 失败、解压越界、payload 缺失、smoke test 失败都不会切换 `current`。已有旧版本时继续保留旧版本。

## 当前仍未落地的完整方案项

以下项属于完整产品方案，但当前代码还没有实现，计划文档不得标成完成：

- 下载进度事件、取消下载、`.part` 临时文件恢复入口、重试/backoff 已落地到 HTTPS artifact 下载器；Range/ETag 精细续传仍按 CDN 能力降级处理。
- 设置页清理旧版本和旧版本保留策略。
- manifest 已支持 channel、minimum app version、sizeBytes、rollback、mirrors/default provider 等生产字段。
- 安装器已支持 zip 和 tar.gz artifact。
- `install.json` 已写入平台、runtime 分组和相对路径元数据。

