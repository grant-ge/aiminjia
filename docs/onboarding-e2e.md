# 接入 AIjia e2e 测试：同事上手指南

> 创建：2026-05-18
> 更新：2026-05-21（迁移到 dual-manifest 方案，pilot 不再进主 Cargo.toml）
> 受众：lotus-app 开发者（**本地开发**，不是 CI）
> 关联：`docs/superpowers/specs/2026-05-21-e2e-toolchain-dual-manifest-design.md`（架构）、`docs/e2e-org1-chat-mainline.md`（CLI 工具规格）

---

## 谁需要读

| 你的角色 | 需要读吗？ |
|---|---|
| 写业务代码（前端 / 后端） | ❌ 不需要，跳过 |
| 写 / 跑意图测试（test-intents） | ✅ 需要 |
| 维护 tauri-pilot 本身 | ✅ 需要 |

普通业务开发：`pnpm tauri:dev` 直接跑即可，**不需要** pilot 仓权限、**不需要**任何额外配置。

## 你将获得什么

跑完这份指南，你的本地环境会有：

1. `tauri-pilot` 命令（在 `~/.cargo/bin/`），自带 `aijia` 子命令组
2. AIjia dev server 用 `pnpm dev:with-pilot` 起来后，能用 `tauri-pilot aijia health-check` / `aijia send` / `aijia wait-reply` 等 16 个命令操作 webview
3. **业务开发零负担**：`pnpm tauri:dev`（不带 e2e）跟以前一样跑，不需要装任何 tauri-pilot 相关的东西

预计耗时：**首次 15–20 分钟**（含 tauri-pilot CLI 编译 + 第一次跑 wrapper）。日常零启动开销。

---

## 前置依赖

| 工具 | 推荐版本 | 检测命令 |
|---|---|---|
| macOS | 任意 | `sw_vers`（Windows 不支持 e2e） |
| Rust | ≥ 1.95.0 | `rustc --version`（不够就 `rustup install 1.95.0`） |
| pnpm | ≥ 8 | `pnpm --version`（已有的 AIjia 开发环境就够了） |
| Tauri 2 系统依赖 | 标准 | `pnpm tauri:dev` 能跑就 OK |

---

## 第一步：申请 tauri-pilot 仓权限（人工，一次性）

tauri-pilot fork 仓在云效私库 **`git@codeup.aliyun.com:renlijia/lotus/tauri-pilot.git`**，找 pzc 加入。

确认权限到位的两个条件：

1. 你的云效账号在 `renlijia/lotus/tauri-pilot` 有读权限
2. 你本机的 SSH 公钥已加到云效（个人 Profile → SSH Keys）

> **不是**上游 `mpiton/tauri-pilot`——上游没有 `aijia` 子命令组。

测试连通性：

```bash
git ls-remote git@codeup.aliyun.com:renlijia/lotus/tauri-pilot.git
# 期望：列出 main 分支 sha
# 报 Permission denied (publickey) → SSH key 没加到云效
# 报 repository not found → 没仓权限，找 pzc 加
```

---

## 第二步：clone tauri-pilot 到 lotus-app **同级目录**

dual-manifest 方案要求 `tauri-pilot` 跟 `lotus-app` 同级 sibling clone。`.e2e/Cargo.toml` 用相对路径 `../../../tauri-pilot/crates/tauri-plugin-pilot` 引用 pilot crate。

```bash
cd /your/IdeaProjects        # ← lotus-app 父目录
git clone git@codeup.aliyun.com:renlijia/lotus/tauri-pilot.git

# 验证目录布局：
# IdeaProjects/
# ├── lotus-app/
# └── tauri-pilot/
#     └── crates/tauri-plugin-pilot/      ← 关键路径
```

验证布局：

```bash
ls -d ../tauri-pilot/crates/tauri-plugin-pilot
# 期望：路径存在
```

---

## 第三步：启动 AIjia 带 e2e 模式

```bash
cd lotus-app
pnpm install                  # 如果之前没装过
pnpm dev:with-pilot
```

`dev:with-pilot` 会自动：

1. **预检（`predev:with-pilot` hook 跑 `scripts/ensure-e2e-prereq.sh`）**：
   - 检测 `jq` 是否在 PATH（bundled runtime 校验脚本需要），没装自动 `brew install`
   - 软检测 ssh-agent identity（不阻塞，pilot 走的是本地 path dep 不需要 ssh）
2. `ensure:runtime`：校验 / 下载内置运行时（Node + Python + uv）
3. `cd src-tauri/.e2e && tauri dev --features e2e`：进入 wrapper crate 目录，触发 build.rs 复制 `capabilities/pilot.json`、cargo 从本地 sibling path 编译 tauri-plugin-pilot、plugin 编进来 + lib.rs 注册

跳过预检：`SKIP_E2E_PREREQ=1 pnpm dev:with-pilot`（不推荐，自己确保环境齐）。

首次跑总时长：~5 分钟（首次 wrapper 全量编译；之后增量 ~20s）。

启动后验证 plugin 真注入了：

```bash
ls /tmp/tauri-pilot-com.aijia.app.sock
# 文件存在 = plugin socket 启动了
```

---

## 第四步（可选）：装 `tauri-pilot` CLI 跑 aijia 命令

**只有当你要跑** `tauri-pilot aijia health-check` / `aijia send` 这些 e2e 命令时才需要。第三步起 app 不需要 CLI。

```bash
cargo install \
  --git git@codeup.aliyun.com:renlijia/lotus/tauri-pilot.git \
  --branch main \
  --bin tauri-pilot \
  tauri-pilot-cli \
  --force
```

- 首次大约 5–8 分钟（cargo clone 仓 + release 编译）
- 装完后 `which tauri-pilot` 应返回 `~/.cargo/bin/tauri-pilot`
- `tauri-pilot aijia --help` 应列出 16 个子命令

更新（aijia 子命令组有改动时）：重新跑同样的 `cargo install` 命令，`--force` 让它覆盖装新版。

跑命令验证：

```bash
tauri-pilot aijia health-check
```

期望输出：

```json
{
  "activeConversationId": "...",
  "hasEditor": true,
  "ok": true,
  "readyState": "complete"
}
```

`ok: true` 说明全套链路通了，可以开始写 / 跑 e2e 脚本。

---

## 架构原理（为什么不直接 `tauri dev`）

主仓 `src-tauri/Cargo.toml` **不含** `tauri-plugin-pilot` dep——让没 codeup 权限的同事和 GitHub CI runner 都能 `pnpm tauri:dev` / `cargo build` 直接通过。

e2e 单独一份 wrapper crate 在 `src-tauri/.e2e/`：

- `src-tauri/.e2e/Cargo.toml` 含 `tauri-plugin-pilot = { path = "../../../tauri-pilot/crates/tauri-plugin-pilot", optional = true }`
- `src-tauri/.e2e/tauri.conf.json` 是主配置的副本，相对路径加一层 `../`
- `src-tauri/.e2e/Cargo.lock` 独立，含 pilot entry，跟主 `Cargo.lock` 完全隔离
- `src-tauri/.e2e/src` / `build.rs` / 资源目录 全部 symlink 回主 `src-tauri/` —— 不复制源码

所以 `pnpm dev:with-pilot` 内部就是 `cd src-tauri/.e2e && tauri dev --features e2e`。

详见 spec：`docs/superpowers/specs/2026-05-21-e2e-toolchain-dual-manifest-design.md`。

---

## 常用命令速查

完整 16 个命令文档：tauri-pilot 仓 `SKILL.md` 的 "App-specific subcommand groups" 章节。

最常用的几个：

```bash
# 状态查询
tauri-pilot aijia where                          # 当前 UI 状态 JSON
tauri-pilot aijia list-sessions | jq '.[0:5]'    # 侧栏前 5 个会话
tauri-pilot aijia ui-message --last 3            # 当前会话最近 3 条消息

# 端到端发消息
tauri-pilot aijia new-task
tauri-pilot aijia type-message "你好"
tauri-pilot aijia send
tauri-pilot aijia wait-reply --timeout 60
tauri-pilot aijia last-reply

# 诊断
tauri-pilot aijia screenshot --label baseline    # PNG 到 /tmp/aijia-e2e-baseline-*.png
```

---

## 铁则：脚本只用 `aijia` 子命令

写 e2e 脚本时**禁止**直接调用 `tauri-pilot click / fill / eval / snapshot / screenshot`：

```bash
❌ tauri-pilot click @e5
✅ tauri-pilot aijia send

❌ tauri-pilot eval 'document.querySelector(...)'
✅ tauri-pilot aijia where    # 已封装常见状态查询
```

理由 + 完整规则见 lotus-app `docs/e2e-org1-chat-mainline.md` 顶部铁则段。

---

## 常见问题

### Q1. `pnpm dev:with-pilot` 报错 `path source '../../../tauri-pilot/...' does not exist`

你没在 `lotus-app` 的同级目录 clone pilot 仓。回到第二步。

### Q2. 我加了个新 dep 到主 `Cargo.toml`，`pnpm dev:with-pilot` 编译失败说缺 dep

`.e2e/Cargo.toml` 是手动维护的主 Cargo.toml 副本。把你新加的 dep 行也复制到 `.e2e/Cargo.toml` 同位置即可（频率低，~月一次）。

### Q3. `tauri-pilot ping` 返回 "No tauri-pilot socket found"

dev server 没起来，或者 dev server 起来了但 plugin 没注入。检查：

1. AIjia 窗口确实开着
2. 用的是 `pnpm dev:with-pilot` 不是 `pnpm tauri:dev`（后者不含 pilot plugin）
3. `ls /tmp/tauri-pilot-*.sock` 应该有一条 `tauri-pilot-com.aijia.app.sock`

### Q4. `tauri-pilot aijia health-check` 报 `window.__aijia missing`

lotus-app 的 `src/main.tsx` 应该有这段（`import.meta.env.DEV` 守卫）：

```ts
if (import.meta.env.DEV) {
  (window as unknown as { __aijia?: unknown }).__aijia = {
    chatStore: useChatStore,
    sessionStore: useSessionStore,
  }
}
```

如果你拉的 lotus-app 版本太老没这段，pull 最新 main。

### Q5. 我想改 aijia 子命令 / 加新命令

直接在 sibling `../tauri-pilot/` 仓里改 → wrapper 跑 `pnpm dev:with-pilot` 自动从本地 path 拉到最新代码（因为是 `path = ...` 引用，不走 git cache）。改完测试 OK 后在 tauri-pilot 仓 commit + push 到云效 main，再让其他人 `git pull`。

CLI binary 想更新（推了新 commit 后），重跑第四步的 `cargo install` 命令带 `--force` 即可。

主要文件：

- `crates/tauri-pilot-cli/src/cli.rs` — clap 定义
- `crates/tauri-pilot-cli/src/aijia.rs` — 16 个命令实现 + 共享 helper
- `crates/tauri-plugin-pilot/js/bridge.js` — 注入 webview 的 JS

完整的 "添加新 aijia 子命令" 流程见 tauri-pilot 仓 `SKILL.md` 的 "Adding a new aijia subcommand" 段。

### Q6. Windows 能跑 e2e 吗？

不能。pilot 工具链当前 macOS-only。Windows 同事跑业务开发不受影响（主 `pnpm tauri:dev` 完全不依赖 pilot）。

---

## 反馈

跑通后或卡住时找 pzc，或在团队群里 @ 一下。
