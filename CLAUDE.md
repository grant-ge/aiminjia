# AIjia — 代码仓库

企业 AI 工作台 Tauri 2.x 桌面应用（React/TS 前端 + Rust 后端 + 捆绑 Python 3.12 数据分析运行时）。

产品名：**AIjia**（元数据/文件名）/ **AI小家**（UI 面向用户）。标识符：`com.aijia.app`。

## 仓库结构（关键部分）

```
code/
├── CLAUDE.md                  # 本文件
├── package.json               # 前端 + version
├── src/                       # React/TS 前端
├── src-tauri/
│   ├── Cargo.toml             # Rust crate + version
│   ├── tauri.conf.json        # Tauri 配置 + version
│   ├── Cargo.lock             # 锁文件 + aijia version
│   ├── python-runtime ->      # 软链（不入库），指向 target/<arch>/release/python-runtime/
│   ├── target/aarch64-apple-darwin/release/python-runtime/   # ARM runtime（不入库）
│   └── target/x86_64-apple-darwin/release/python-runtime/    # Intel runtime（不入库）
├── scripts/
│   ├── setup-python.sh/.ps1       # 下载 python-build-standalone + 装依赖
│   ├── setup-playwright.sh/.ps1   # Playwright 浏览器 sidecar
│   ├── ci-upload-windows.py       # CI 用：Windows runner 传 OSS
│   ├── ci-upload-macos.py         # CI 用：macOS runner 传 OSS（接 arch 参数）
│   ├── ci-finalize.py             # CI 用：Ubuntu runner 生成 update.json
│   ├── bump-homebrew.py           # 本地：发版后更新 Homebrew cask
│   └── upload-x64.py              # 本地可选：Intel 本地构建 + 传 OSS
└── .github/workflows/
    └── build-desktop.yml          # tag push → CI 构建 + 直传 OSS + update.json
```

设计文档在姊妹仓库 `../docs/`（Gitee: `inkess/team-docs`）。架构权威在 `docs/agent-architecture.md`。

## 开发命令

```bash
pnpm install                 # 首次或依赖变更
bash scripts/setup-python.sh # 首次：装本机 arch 的 bundled Python（⚠️ 见下）
pnpm tauri dev               # 本地 dev
pnpm tauri build             # 本地打当前架构包（发版通常不用 — CI 全自动）
pnpm lint                    # ESLint
pnpm build                   # 仅前端 bundle
```

## 发布流程（权威 · 自 v0.4.14 起）

推 tag → CI 全自动 → 本地跑一行 Homebrew bump。就这样。

### 1. 改版本号（4 处，都要改）

- `package.json` → `version`
- `src-tauri/tauri.conf.json` → `version`
- `src-tauri/Cargo.toml` → `version`
- `src-tauri/Cargo.lock` → `[package] name = "aijia"` 下的 `version`

### 2. Commit + tag + push

```bash
git commit -am "release: vX.Y.Z"
git tag vX.Y.Z
git push codeup main && git push codeup vX.Y.Z
git push origin main && git push origin vX.Y.Z   # 这步触发 CI
```

### 3. CI 自动做（`.github/workflows/build-desktop.yml`）

| Job | Runner | 脚本 | 产出 |
|-----|--------|------|------|
| `build (windows-latest)` | Windows | `ci-upload-windows.py` | `.exe` + `.sig` → OSS |
| `build (macos-14)` | macOS arm64 | `ci-upload-macos.py` | `.dmg` + `.app.tar.gz` + `.sig` → OSS |
| `finalize` | Ubuntu | `ci-finalize.py` | `update.json` → OSS |

`build` 阶段并行；`finalize` 依赖两个 `build` 都成功。全程约 22-25 分钟。

Windows OSS 上传的 PowerShell 步骤开启 `PYTHONUTF8=1` + 所有脚本 ASCII 打印 + 每步 `$LASTEXITCODE` 检查，避免历史上 charmap codec 崩溃。

### 4. 等 CI 绿（可监控）

```bash
gh run watch $(gh run list -R grant-ge/aiminjia -w "Build Desktop Apps" -L 1 --json databaseId --jq '.[0].databaseId') \
  -R grant-ge/aiminjia --exit-status
```

### 5. Homebrew bump（本地唯一手动步骤）

```bash
python3 scripts/bump-homebrew.py X.Y.Z
```

### 6. 验证

```bash
curl -sI https://lotus.renlijia.com/aijia/vX.Y.Z/AIjia_X.Y.Z_aarch64.dmg        # 200
curl -sI https://lotus.renlijia.com/aijia/vX.Y.Z/AIjia_X.Y.Z_x64-setup.exe      # 200
curl -s  https://lotus.renlijia.com/aijia/update.json | jq '.version, .platforms | keys'
```

## 可选：macOS Intel 本地补包

CI 不打 Intel（GitHub 已下线 native Intel runner）。Intel 用户少，仅需要时在本机补：

```bash
# 1) 软链切到 Intel runtime（target/x86_64-apple-darwin/release/python-runtime 必须已就位）
rm src-tauri/python-runtime
(cd src-tauri && ln -sfn target/x86_64-apple-darwin/release/python-runtime python-runtime)
file src-tauri/python-runtime/bin/python3.12   # 验证 x86_64

# 2) 签名 env
export TAURI_SIGNING_PRIVATE_KEY=$(cat ~/.tauri/aijia.key)
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=$(security find-generic-password -s aijia-tauri-signer -a password -w)

# 3) Cross-compile（Rosetta 下 pip 也走 x86_64）
pnpm tauri build --target x86_64-apple-darwin --bundles app
hdiutil create -volname "AIjia" \
  -srcfolder src-tauri/target/x86_64-apple-darwin/release/bundle/macos/AIjia.app \
  -ov -format UDZO \
  src-tauri/target/x86_64-apple-darwin/release/bundle/dmg/AIjia_X.Y.Z_x64.dmg

# 4) 上传 + 自动 merge 到 update.json
python3 scripts/upload-x64.py X.Y.Z

# 5) 切回 ARM（重要！本地 pnpm tauri dev 用 ARM）
rm src-tauri/python-runtime
(cd src-tauri && ln -sfn target/aarch64-apple-darwin/release/python-runtime python-runtime)
```

## Python Runtime 双架构软链规范

`src-tauri/python-runtime` 是**软链**，指向 `target/<arch>-apple-darwin/release/python-runtime/` 里的真实目录。两份 runtime 常驻 `target/<arch>/`，切换只改软链 —— 切架构零下载。

**首次设置**（两份都要建一次）：

```bash
# ARM
rm -rf src-tauri/python-runtime
PIP_INDEX_URL=https://pypi.tuna.tsinghua.edu.cn/simple \
PIP_TRUSTED_HOST=pypi.tuna.tsinghua.edu.cn \
bash scripts/setup-python.sh
mkdir -p src-tauri/target/aarch64-apple-darwin/release
mv src-tauri/python-runtime src-tauri/target/aarch64-apple-darwin/release/python-runtime

# Intel
PIP_INDEX_URL=https://pypi.tuna.tsinghua.edu.cn/simple \
PIP_TRUSTED_HOST=pypi.tuna.tsinghua.edu.cn \
arch -x86_64 bash scripts/setup-python.sh
mkdir -p src-tauri/target/x86_64-apple-darwin/release
mv src-tauri/python-runtime src-tauri/target/x86_64-apple-darwin/release/python-runtime

# 默认软链到 ARM
(cd src-tauri && ln -sfn target/aarch64-apple-darwin/release/python-runtime python-runtime)
```

设清华镜像环境变量是因为默认 PyPI 在国内走代理常超时。

## 关键设计决策

- Tauri 2.x：Ed25519 signed auto-updater（密钥 `~/.tauri/aijia.key`，GitHub Secrets `TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)`）
- 数据存储：SQLite + AES-256-GCM（敏感字段）
- i18n：react-i18next，`src/i18n/{zh-CN,en-US}.json`，默认 zh-CN
- OSS：阿里云 `lotus-releases` bucket，前缀 `aijia/`，CDN `https://lotus.renlijia.com`
- Homebrew：`grant-ge/homebrew-tap` 下 `Casks/aijia.rb`，`on_arm` / `on_intel` 分架构 URL
- GitHub Secrets（4 个，都已就位）：`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, `OSS_ACCESS_KEY_ID`, `OSS_ACCESS_KEY_SECRET`

## 数据存储位置

- 本地数据：macOS `~/Library/Application Support/com.aijia.app/`，Windows `%APPDATA%\com.aijia.app\`
- 签名密钥（本地开发/Intel 构建）：`~/.tauri/aijia.key` + Keychain `aijia-tauri-signer`
- OSS 凭证（本地 `upload-x64.py`/`bump-homebrew.py` 用）：Keychain `aijia-oss`，或环境变量 `OSS_ACCESS_KEY_ID` / `OSS_ACCESS_KEY_SECRET`

## Git Remotes

- `origin` → `github.com:grant-ge/aiminjia.git`（公开，CI 运行于此）
- `codeup` → `codeup.aliyun.com:renlijia/lotus/lotus-app.git`（国内镜像）

两个都要推。tag 只触发 `origin` 的 CI。
