# E2E Release Build —— 打带 plugin-pilot 的 release 包

**目的**：发个**带意图测试接口**的 release-mode `.app` 给 QA / 自测用，跟正式 release 包（不带 pilot）分离。

**适用场景**：
- 给 QA 装一个跟正式包接近的安装包，但**能跑 `tauri-pilot aijia ...` 意图测试**
- 想验证「release profile 优化下意图测试还能不能通」
- 不需要 dev server 跑着、cargo / vite 不占资源

**不适用**：正式对外发版（用 `scripts/release.py` 那套）。本文档产出的包是**内部 QA 用**，未签名 / 未公证 / 不走 OSS / 不触发自动更新。

---

## 一条命令出包

```bash
pnpm build:with-pilot
```

产物：

| 文件 | 用途 |
|---|---|
| `src-tauri/target/release/bundle/macos/AIjia.app` | macOS .app（205 MB，含 bundled runtime + plugin-pilot） |
| `src-tauri/target/release/bundle/macos/AIjia.app.tar.gz` | updater tarball（77 MB，可直接 scp 给 QA） |

末尾会报 `Error A public key has been found, but no private key. Make sure to set TAURI_SIGNING_PRIVATE_KEY environment variable` —— 这是 tauri 给 updater tarball 签名的步骤失败，**不影响 .app 包本身**，QA 用 .app 即可。

## QA 怎么用

```bash
# 1. 把 .app 拖到 /Applications（或任何位置）

# 2. 首次启动绕过 Gatekeeper：右键 .app → 打开 → 接受警告
#    包未签名 + 未公证，macOS 会拦"未识别的开发者"

# 3. 启动后验证 e2e 接口
ls /tmp/tauri-pilot-com.aijia.app.sock           # 应该存在
tauri-pilot aijia health-check --json            # 应该 ok=true

# 4. 跑意图测试（同 dev 模式，命令完全一致）
tauri-pilot aijia new-task
tauri-pilot aijia type-message "hi"
tauri-pilot aijia send
tauri-pilot aijia wait-reply --timeout 60
tauri-pilot aijia last-reply --json
```

## 为什么 release 包默认不带 pilot

`src-tauri/Cargo.toml`：
```toml
tauri-plugin-pilot = { git = "https://github.com/panzhenchao/tauri-pilot.git", branch = "main", optional = true }

[features]
default = []
e2e = ["dep:tauri-plugin-pilot"]
```

- `optional = true` —— 默认 `pnpm tauri build` **不带** pilot 代码
- `e2e = ["dep:tauri-plugin-pilot"]` —— 显式 `--features e2e` 时才把 pilot 编进去

主仓 `src/lib.rs` 用 `#[cfg(feature = "e2e")]` gate 注册 plugin。没开 feature → plugin 不注册 → socket 不监听。

## `build:with-pilot` 实际做了 4 件事

`package.json`：
```json
"build:with-pilot": "VITE_E2E_ENABLED=true tauri build --features e2e --config src-tauri/tauri.conf.e2e.json"
```

| 步骤 | 作用 |
|---|---|
| `--features e2e` | 让 cargo 编进 plugin-pilot crate + 激活主 lib.rs 的 `#[cfg(feature="e2e")]` gate |
| `--config src-tauri/tauri.conf.e2e.json` | overlay 主 `tauri.conf.json`，给 CSP 的 `script-src` 加 `unsafe-eval`（pilot 用 `eval()` 评估 JS） |
| `VITE_E2E_ENABLED=true` | 前端 `main.tsx` 看到这变量后会注入 `window.__aijia` 全局对象（`aijia` CLI 通过它读 store 状态） |
| `tauri build` 本身 | 走 release profile：LTO / strip / codegen-units=1，编译 760 个 crate，约 6-8 分钟 |

## 跟正式 release 的差异

|  | 正式 release（`pnpm tauri build`） | e2e release（`pnpm build:with-pilot`） |
|---|---|---|
| plugin-pilot 代码 | 不编入 | 编入 |
| CSP `unsafe-eval` | ❌ 严格 | ✅ 放开 |
| `window.__aijia` 全局 | 不注入 | 注入 |
| 签名 / 公证 | 走 `build-and-sign-macos.sh` | 不签 |
| OSS 上传 | 走 `sign-and-upload-macos.sh` | 不上传 |
| 自动更新 | `latest/` + `update.json` | 不触发 |
| socket | 不存在 | `/tmp/tauri-pilot-com.aijia.app.sock` |

## 历史踩坑（按时间）

下面这些问题在 2026-05-25 把 dual-manifest 拆掉、pilot 公开到 GitHub 时全部踩过，**留作经验**。如果你重新设计这套工具链，知道这些点能少走弯路。

### 1. pilot 的 `debug_assertions` 守门

mpiton 原版 pilot 在 `lib.rs` 用 `#[cfg(all(any(unix, windows), debug_assertions))]` gate socket server——release profile 下 `debug_assertions` 为 false，socket **不会启动**。

我们 fork（`panzhenchao/tauri-pilot`）的 commit `bc7cb7c` 把这条 gate 去掉，改为只 `#[cfg(any(unix, windows))]`。这样 release build 也能起 socket。

### 2. CSP 拒绝 `unsafe-eval`

pilot 的 `evaluate_script` 通过 `eval(string)` 把 CLI 传来的 JS 字符串当代码执行。主仓 `tauri.conf.json` 的 `csp` 字段没放行 `unsafe-eval`，release 模式下 webview 拒执行 → `tauri-pilot aijia health-check` 报 `Refused to evaluate a string as JavaScript`。

修法：`tauri.conf.e2e.json` overlay 只覆盖 `app.security.csp`，加 `'unsafe-eval'`。正式 release 用主 conf 不动。

### 3. `window.__aijia` 被 `import.meta.env.DEV` 守门

`src/main.tsx` 原本用 `if (import.meta.env.DEV) { window.__aijia = ... }`——release 模式 DEV=false，全局对象不存在。

`aijia` CLI 的很多命令通过 `window.__aijia.chatStore` / `.authStore` 读 zustand state，没这对象就 `health-check` 报 `window.__aijia missing`。

修法：`main.tsx` 改为 `if (import.meta.env.DEV || import.meta.env.VITE_E2E_ENABLED === 'true')`，build 命令传 `VITE_E2E_ENABLED=true` 环境变量。

### 4. dual-manifest 已经废了（2026-05-25）

历史上为了「同事没 codeup 权限也能 build 主仓」搞过一套 `src-tauri/.e2e/` wrapper crate（spec `docs/superpowers/specs/2026-05-21-e2e-toolchain-dual-manifest-design.md`）。它的核心机制是：

- 主 `Cargo.toml` 不引 pilot（避免 codeup SSH 鉴权）
- `.e2e/Cargo.toml` 副本 + 加 pilot path dep
- `.e2e/` 下用 symlink 复用主仓源码

**但 dual-manifest 漏洞百出**：两份 Cargo.toml 会 drift（一边加 wa-rs / tokio-websockets / sysproxy 另一边没跟着）、两份 Cargo.lock 独立 resolve（一边 tauri 2.10.2 一边 2.11.2）、版本号 drift（0.5.26 vs 0.5.29）。release build 比 dev 严格，drift 全暴露出来。

**2026-05-25 拆掉**：pilot 公开到 GitHub（`panzhenchao/tauri-pilot`），主 `Cargo.toml` 直接引 `git = "https://github.com/panzhenchao/tauri-pilot.git"`。`src-tauri/.e2e/` 整目录删除。从此只一份 manifest 一份 lock。

### 5. pilot dep 要 `tauri-plugin ^2.6.1`

pilot 0.5.2 依赖 `tauri-plugin ^2.6.1`，但 lotus-app 主仓 Cargo.lock 一开始锁在 `tauri-plugin 2.5.3`（间接由 `tauri-plugin-dialog 2.6.0` 锁）。`cargo check --features e2e` 报 dep 冲突。

修法：`cargo update -p tauri-plugin`。`tauri-plugin-dialog 2.6.0` 接受 `tauri-plugin ^2.4` 范围、升到 2.6.2 没问题。

### 6. `EmployeeDrawer.tsx` 漏传 `confirmLabel`（已修）

i18n PR `d68eab57` 把 `window.confirm()` 改成 `requestConfirm()` 时，`EmployeeDrawer.tsx:252/276` 两处漏传 `confirmLabel`。dev 模式 `tsc --noEmit` 报 warn 不阻断，release 的 `tsc -b && vite build` 升级成 error。

修法：补两处 `confirmLabel: t('common.confirm')`。

---

## 未来的事

- **签名 / 公证 / OSS 上传**：现在 e2e release 包未签名，QA 装机需要手动绕过 Gatekeeper。如果未来 QA 数量多，可能需要给 e2e 包也签名（但**不公证**，公证是面向公网的）。
- **Windows / Linux**：当前只验证过 macOS arm64。Windows 上 pilot 走命名管道 `\\.\pipe\tauri-pilot-{identifier}`，应该能跑，但**没测过 release 包路径**。
- **自动化**：现在 `pnpm build:with-pilot` 是手动跑。如果发版频次高，加个 `scripts/build-e2e-release.sh` 把它跟 prepare-bundled-runtime / 版本号同步 / 产物归档串起来。
