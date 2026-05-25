# 发布流程（权威 · 自 v0.5.23 起，本地构建 + 一键 Windows）

**架构：macOS 本地全流程 + Windows GitHub-hosted 构建未签名包 + 本地一键签名上传**。

- **macOS arm64 + x86_64**：在你的 Mac 上 `build-and-sign-macos.sh` 串行 build → codesign → notarize → staple → tauri signer → 上传 OSS。串行的原因是 `setup-dws.sh` 切换 runtime symlink，并发会冲突。
- **Windows x64**：tag 触发 `windows-latest` runner 跑 unsigned 构建 → 产物上传到 OSS staging（`aijia/staging/unsigned/v{ver}/`，公开 CDN URL）→ 在你的 Windows 机器上跑 `release-windows.ps1` 一键完成：拉 staging exe → signtool 签名（带 timestamp）→ tauri signer 生成 `.sig` → Node + `ali-oss` 上传 → 清理 staging。

## 内置运行时（自 0.5.24 起）

发版前**必须**先跑 `scripts/prepare-bundled-runtime.{sh,ps1}`，把 Node 20.18 / Python 3.12.7 / uv 0.4.27 打入 `src-tauri/resources/runtime/<platform>/`。Tauri build 把目录复制进安装包（~85MB 增量），用户首启完全离线可用。

- **入口脚本**：mac 走 `scripts/build-and-sign-macos.sh` 内部自动按 arch 调 `prepare-bundled-runtime.sh PLATFORM=darwin-{arm64,x64}`；Windows CI `.github/workflows/build-desktop.yml` 的 build job 自动跑 `prepare-bundled-runtime.ps1`。
- **上游源**：`scripts/runtime-sources.json` pin 了 Node（nodejs.org）、Python（mac 用 python-build-standalone install_only，win 用官方 embeddable zip）、uv（astral-sh release）的 URL + sha256。升级运行时改 `bundleVersion` + 各组件 version + 9 个 sha256。
- **缓存**：`.runtime-cache/`（已 gitignore）按文件名缓存下载产物，CI 也 cache 这个目录（`actions/cache@v4` key on `hashFiles('scripts/runtime-sources.json')`）。
- **解析链**：启动期 `BundledRuntimeResolver`（reads `app.path().resource_dir()/runtime/<platform>/`）→ `InstalledRuntimeResolver`（`~/.renlijia/runtimes/renlijia-primary-runtime/current`，OSS 升级路径）→ on-demand OSS download（兜底）。前两个任一成功即跳过 OSS。详见 `src-tauri/src/runtime/dependencies/{bundled_resolver,chain_resolver,manager}.rs`。
- **mac 签名**：现有 inside-out `find -type f` + `file ... Mach-O` 自动覆盖 `Contents/Resources/runtime/` 下所有二进制（node/python3/uv/uvx/libpython3.12.dylib + lib-dynload `.so`）。`sign-and-upload-macos.sh` 在签名后**审计** runtime/ 下每个 Mach-O 是否带 `flags=...runtime`（hardened），漏一个则 fail，防止 notarization 拒签。
- **Windows 签名**：`release-windows.ps1` 只签外层 NSIS 安装包，**不**单独签 `resources\runtime\` 内嵌的 `node.exe / python.exe / uv.exe / uvx.exe`。SmartScreen 只校验外层签名，nested PE 不展示在用户首次启动的警告里——因此当前是有意 trade-off（每个 nested exe 单独走 signtool + timestamp 会增加 ~30s 发版时间）。如果将来用户报告 "Unknown publisher" 提示在 PowerShell 直接执行内嵌 exe 时出现，再补 signtool 遍历。
- **诊断**：Settings → 运行时 显示 `activeResolver`、内置版本号、`node/python/uv --version` 实时输出 + 一键重检（`runtime_diagnostics` Tauri 命令 → `src/components/settings/panels/RuntimePanel.tsx`）。
- **Spec/Plan**：`docs/superpowers/plans/2026-05-13-bundle-runtime-into-installer.md`。

## 三种包

| 类型 | 签名 | 来源 | 用途 |
|------|------|------|------|
| **Beta** | 已签名 | 本地签名后上传到 `aijia/beta/v{x}/` | 内测验证，不触发自动更新 |
| **Release** | 已签名 | 本地签名后上传到 `aijia/v{x}/` + `latest/` | 正式版，触发自动更新 |
| ~~Dev~~ | ~~未签名~~ | ~~已弃用~~ | ~~CI 已不再产出 dev 包~~ |

**下载页**：https://lotus.renlijia.com/aijia/downloads.html

## 工作流（一次性命令）

```bash
# 1. 启动版本（在 mac 上）
python3 scripts/release.py start              # bump base 版本号（X.Y.Z）
python3 scripts/release.py beta               # bump 到 X.Y.Z-beta.N + 推 tag → 触发 Windows CI

# 2. macOS（本地，串行 arm64 → x86_64，~35min）
bash scripts/build-and-sign-macos.sh X.Y.Z-beta.N beta
# 单架构重跑（其中一边失败时）：ARCH=x86_64 bash scripts/build-and-sign-macos.sh ...

# 3. Windows（在 Windows 机器上，一条命令，首次问 4 个值之后全自动）
.\scripts\release-windows.ps1 -Version X.Y.Z-beta.N -Type beta

# 4. 验证（mac 上跑，~30s）
bash scripts/verify-release.sh X.Y.Z-beta.N beta

# 5. 测试通过后正式发布（同上，beta 改为 release）
python3 scripts/release.py test-passed
python3 scripts/release.py release            # bump 回 X.Y.Z + 推 release tag
bash scripts/build-and-sign-macos.sh X.Y.Z release
.\scripts\release-windows.ps1 -Version X.Y.Z -Type release
bash scripts/verify-release.sh X.Y.Z release
python3 scripts/release.py finalize           # 生成 update.json → 自动更新生效

# 6. 收尾（finalize 之后）
python3 scripts/bump-homebrew.py X.Y.Z        # 同步 grant-ge/homebrew-tap
cd ~/lotus && ./scripts/update-changelog.sh desktop X.Y.Z
# → 填 changelog.json → 部署 home → push → 自动推钉钉群（带 "AI小家"）
```

## 关键脚本

| 脚本 | 平台 | 职责 |
|------|------|------|
| `scripts/release.py` | mac | 交互式流程守卫：bump 版本 / 推 tag / 强制顺序 / finalize |
| `scripts/build-and-sign-macos.sh` | mac | macOS 串行 arm64 → x86_64：build + sign + notarize + upload，幂等可重跑 |
| `scripts/sign-and-upload-macos.sh` | mac | 单架构 sign + notarize + upload（被上面脚本调用，也可单独跑） |
| `scripts/release-windows.ps1` | win | Windows 一键流程：staging 下载 → signtool 签 → tauri sig → ali-oss 上传 → 清理 |
| `scripts/ci-upload-windows.mjs` | win | Node + ali-oss 上传（替代旧 Python 版） |
| `scripts/ci-cleanup-staging.mjs` | win | 发布后删除 OSS staging 文件 |
| `scripts/verify-release.sh` | mac | 发版后全量验证（OSS 可达性 + 签名 + 公证 + spctl） |

## macOS 签名脚本的关键点

1. **inside-out 逐文件 codesign**：`--deep` 在 macOS 11+ 不可靠，会漏签 `Contents/Resources/dws` 等嵌套二进制。脚本对每个 Mach-O 二进制独立签，每个都带 `--timestamp --options runtime`。
2. **DMG 必须由脚本构造（自 v0.5.25 起）**：tauri.conf.json `bundle.targets` 已去掉 `dmg`，只保留 `["nsis", "app"]`。理由：tauri 2.x 的 `bundle_dmg.sh` 调用约定与系统装的 Homebrew `create-dmg` 1.2.3 不匹配，每次发版都失败。`sign-and-upload-macos.sh` 的 Step 1b 用 `hdiutil create -volname "AIjia $VERSION" -fs HFS+ -format UDZO` 从签好的 .app + `/Applications` symlink 构造 DMG，再 codesign 一次。三个参数都必须显式：① volname 带版本号避冲突；② **`-fs HFS+`** —— APFS DMG 头部会浪费 ~30MB 元数据（v0.5.24 因为漏了这个参数，DMG 从 92MB 涨到 122MB）；③ `-format UDZO` zlib 压缩。脚本入口前置自动清理 `/Volumes/AIjia*` 残留挂载和 `hdiutil info` 里的 aijia images，防止"Operation not permitted"。
3. **并行 notarize（自 v0.5.25 起）**：DMG 和 .app 同时提交 Apple notary，每个独立 60min timeout，submission id + rc 写盘后台监控；任一失败保留 tmp dir 调试。串行版（v0.5.24）总耗时被 Apple 端排队叠加；并行版接近单次延迟。**自重试（自 v0.5.25-beta.3 起）**：notarize 函数包了 3 次重试循环，仅当 log 含 `abortedUpload|connectTimeout|HTTPClient|connection.*reset|EOF` 这类瞬时网络错误时才重试，Apple 业务拒绝（`Invalid`/`Rejected`）立即 fail。两次重试间隔 30s。背景：Apple notary 把 zip/dmg 上传到 S3 `notary-submissions-prod` bucket 偶尔会 abort，重试基本就好。
3. **幂等检测**：每步前先 probe（`codesign -dv` 看 `flags=runtime` + `Authority=Developer ID Application`；`xcrun stapler validate` 看是否已 stapled）。重跑只跑没完成的步骤。
4. **签名前 .app 版本预检（v0.5.24+）**：codesign 前用 `PlistBuddy` 读 `CFBundleShortVersionString` 和传入 `$VERSION` 比对（兼容 `-beta.N` 后缀去除），不一致 fail 并提示 `CLEAN_BUILD=1 bash scripts/build-and-sign-macos.sh ...`。同时挂载 DMG 抽查内副本版本，不一致只 warn（Step 2 会重建 DMG，非致命）。防止上一次失败的 build 在 `target/` 留下旧 .app + 幂等 probe 跳过签名 → 把上版本当新版本签上去发出去。`build-and-sign-macos.sh` 提供 `CLEAN_BUILD=1` 环境变量入口跑 `cargo clean`。
5. **手动重启 sign-and-upload 注意事项**：通过 `build-and-sign-macos.sh` 串行调用是最可靠路径；如果需要在中间步骤之后单独重跑 `sign-and-upload-macos.sh`（例如 notarize 中断后接力 staple），**必须前台跑或用 `caffeinate -is bash scripts/sign-and-upload-macos.sh ...`**。`nohup ... &` + `disown` 在 macOS 下父 bash 退出仍可能 SIGHUP 杀子进程——曾在 v0.5.24 发版时导致 .app notarize 完成后脚本死掉、需手动接力 staple/sig/upload。

## Windows 一键脚本的关键点

1. **零 Python 依赖**：用 Node + `ali-oss` SDK 上传 OSS。避开了 Windows 上 Microsoft Store python.exe stub 的常见坑（exit 9009）。
2. **凭据持久化**：4 个值（cert thumbprint / OSS key id / OSS secret / tauri key 密码）首次输入后存进 Windows Credential Manager（cmdkey + Win32 CredRead），之后只需要传 `-Version` + `-Type`。`-Reconfigure` 重新输入。
3. **signtool 自动化**：脚本自动拼对的命令 `signtool sign /v /fd sha256 /sha1 <thumbprint> /tr <timestamp-url> /td sha256 <exe>`。不会再漏 `/tr` 导致无 timestamp 签名。
4. **Tauri key 走文件路径**：`tauri signer sign -k <file>`，不走环境变量（避免 PowerShell 传 base64 给子进程时引入空白字符）。
5. **公开 staging URL**：CI 上传到 `aijia/staging/unsigned/v{ver}/`，CDN 公开可下载（不需要 GitHub token / gh CLI），任何 Windows 机器只要 `git pull` 就能跑发版。
6. **下载页刷新**：上传 + 清理 staging 之后调 `ci-generate-download-page.py` 重生成 `aijia/downloads.html`。背景：macOS `build-and-sign-macos.sh` 在自己跑完时刷一次下载页，但那时还没有 Windows exe；Windows 在 macOS 之后跑，必须自己再刷一次，否则下载页只列 macOS 4 个产物（v0.5.26-beta.6 实战踩过）。失败时只 warn 不阻断,因为产物已经上传成功。

## 签名机环境要求

**macOS**（检查：`bash scripts/setup-runner-macos.sh`）：
- Developer ID Application 证书已导入 login keychain
- 环境变量：`APPLE_ID`, `APPLE_PASSWORD`（App-Specific Password）, `APPLE_TEAM_ID`
- 环境变量：`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- 环境变量：`OSS_ACCESS_KEY_ID`, `OSS_ACCESS_KEY_SECRET`
- 推荐：把上面 7 个变量集中写到 `.env.local.aijia`（已加 .gitignore，chmod 600），构建脚本 `source` 一下

**Windows**（检查：`.\scripts\setup-runner-windows.ps1`）：
- SimpleSign + EV 硬件 token（GUI 签名工具）或 signtool.exe（Windows SDK）
- Node.js（项目本来就要）
- `$HOME\.tauri\aijia.key`（从 macOS 机器拷过来）
- 首次跑 `release-windows.ps1` 时交互式存入 Credential Manager

## GitHub Secrets（CI 构建用，仅 Windows CI 需要）

| Secret | 用途 |
|--------|------|
| `TAURI_SIGNING_PRIVATE_KEY` | Tauri updater Ed25519 密钥 |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Ed25519 密钥密码 |
| `OSS_ACCESS_KEY_ID` | 阿里云 OSS（staging 上传 + 下载页生成） |
| `OSS_ACCESS_KEY_SECRET` | 阿里云 OSS |

注：Apple 签名 / Windows 签名相关 secrets 不再需要（签名都在本地做）。

## OSS 路径规范

```
aijia/
├── staging/unsigned/v{ver}/     # CI 上传的 Windows 未签名包（CDN 公开，签完会被清理）
├── beta/v{ver}/                 # Beta 测试版（已签名）
│   ├── AIjia_{ver}_aarch64.dmg
│   ├── AIjia_{ver}_x64.dmg
│   ├── AIjia.app.tar.gz + .sig          # ARM64 updater
│   ├── AIjia_x64.app.tar.gz + .sig      # Intel updater
│   ├── AIjia_{ver}_x64-setup.exe + .sig # Windows
├── v{ver}/                      # 正式版（已签名）
├── latest/                      # 正式版最新下载入口
├── downloads.html               # 下载页（CI 自动生成）
└── update.json                  # Tauri 自动更新清单（仅正式版）
```

## CI Workflows

| Workflow | 触发 | 作用 |
|----------|------|------|
| `build-desktop.yml` | `beta-v*` / `v*` tag / manual | **仅 Windows**：GitHub-hosted 构建未签名 exe → 上传到 OSS staging + 生成下载页 |
| `finalize-release.yml` | manual（输入 version） | 生成 update.json |
| `ci.yml` | push main / PR | Rust + TS 类型检查 + lint |
