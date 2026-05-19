# 接入 AIjia e2e 测试：同事上手指南

> 创建：2026-05-18
> 更新：2026-05-19（tauri-pilot 改从云效 git dependency 拉，不再要求 sibling clone）
> 受众：lotus-app 开发者（**本地开发**，不是 CI）
> 关联：`docs/e2e-org1-chat-mainline.md`（CLI 工具规格）、`docs/e2e-testing-decisions.md`（选型决策）

---

## 你将获得什么

跑完这份指南，你的本地环境会有：

1. 一个能用的 `tauri-pilot` 命令（在 `~/.cargo/bin/`），自带 `aijia` 子命令组
2. AIjia dev server 用 `pnpm dev:with-pilot` 起来后，能用 `tauri-pilot aijia health-check` / `aijia send` / `aijia wait-reply` 等 16 个命令操作 webview
3. **业务开发零负担**：`pnpm tauri:dev`（不带 e2e）跟以前一样跑，不需要装任何 tauri-pilot 相关的东西

预计耗时：**首次 15–20 分钟**（含 Rust 工具链下载、tauri-pilot 编译）。日常使���零启动开销。

---

## 前置依赖

| 工具 | 推荐版本 | 检测命令 |
|---|---|---|
| macOS | 任意 | `sw_vers` |
| Rust | ≥ 1.95.0 | `rustc --version`（不够就 `rustup install 1.95.0`） |
| pnpm | ≥ 8 | `pnpm --version`（已有的 AIjia 开发环境就够了） |
| Tauri 2 系统依赖 | 标准 | `pnpm tauri:dev` 能跑就 OK |

---

## 第一步：申请 tauri-pilot 仓权限（人工，一次性）

tauri-pilot fork 仓在云效私库 **`git@codeup.aliyun.com:renlijia/lotus/tauri-pilot.git`**，找 pzc 加入。

确认权限到位的两个条件（这两个 agent 替不了你）：

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

通过这一步之后，**剩下的环境配置 agent 全自动**（`pnpm dev:with-pilot` 的 `predev` hook 会跑 `scripts/ensure-e2e-prereq.sh`）。

---

## 第二步：启动 AIjia 带 e2e 模式（一行命令）

```bash
cd lotus-app
pnpm install                  # 如果之前没装过
pnpm dev:with-pilot
```

`dev:with-pilot` 会自动：

1. **预检（`predev:with-pilot` hook 跑 `scripts/ensure-e2e-prereq.sh`）**：
   - 检测 `~/.cargo/config.toml` 是否含 `[net] git-fetch-with-cli = true`，没有自动加（让 cargo 走系统 git，避开 libgit2 不识别 ed25519 / macOS keychain key 的问题）
   - 检测 `jq` 是否在 PATH（bundled runtime 校验脚本需要），没装自动 `brew install` (macOS) / `apt-get install` (Linux)
   - 软检测 ssh-agent 是否有 identity；没有也不阻塞（macOS keychain 整合的 key 是 lazy load 的）
2. `ensure:runtime`：校验 / 下载内置运行时（Node + Python + uv）到 `src-tauri/resources/runtime/<platform>/`
3. `tauri dev --features e2e`：触发 build.rs 复制 `capabilities/pilot.json`、cargo 从云效拉 tauri-plugin-pilot 源码（首次 ~30s，之后走 `~/.cargo/git/` 缓存）、plugin 编进来 + lib.rs 注册

跳过预检：`SKIP_E2E_PREREQ=1 pnpm dev:with-pilot`（不推荐，自己确保环境齐）。

首次跑总时长：~10 分钟（Rust 编译占大头）。

启动后验证 plugin 真注入了：

```bash
ls /tmp/tauri-pilot-com.aijia.app.sock
# 文件存在 = plugin socket 启动了
```

---

## 第三步（可选）：装 `tauri-pilot` CLI 跑 aijia 命令

**只有当你要跑** `tauri-pilot aijia health-check` / `aijia send` 这些 e2e 命令时才需要。第二步起 app 不需要 CLI。

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

## 第四步：常用命令速查

完整 16 个命令文档：tauri-pilot 仓 `SKILL.md`（云效在线浏览 `git@codeup.aliyun.com:renlijia/lotus/tauri-pilot` → SKILL.md）的 "App-specific subcommand groups" 章节。

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

### Q1. `cargo install` 卡在某个 crate 半天没动

进入 tauri-pilot 仓跑 `cargo build --bin tauri-pilot --release` 直接看到底卡在哪——一般是 wry / tauri 这种大依赖编译慢，等就行。如果是网络拉不到，配 cargo 镜像（rsproxy.cn 或 ustc）。

### Q2. `tauri-pilot ping` 返回 "No tauri-pilot socket found"

dev server 没起来，或者 dev server 起来了但 plugin 没注入。检查：

1. AIjia 窗口确实开着
2. lotus-app 是 dev 构建（`pnpm tauri:dev`），不是 release——release 包会把整个 plugin 剔除
3. `ls /tmp/tauri-pilot-*.sock` 应该有一条 `tauri-pilot-com.aijia.app.sock`

### Q3. `tauri-pilot aijia health-check` 报 `window.__aijia missing`

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

### Q4. `pnpm dev:with-pilot` 失败：`failed to authenticate when downloading repository` / `no authentication methods succeeded`

正常情况下不会发生 —— `predev:with-pilot` hook 已自动写入 `~/.cargo/config.toml` 的 `git-fetch-with-cli = true`。如果还看到这个错，说明：

1. 跑时设了 `SKIP_E2E_PREREQ=1` 跳过自检 → 取消这个环境变量重新跑
2. `~/.cargo/config.toml` 被你或其他工具改回去了 → 手动确认它含 `[net] git-fetch-with-cli = true`
3. 真有 ssh key 问题 → 直接测系统 git：

```bash
git ls-remote git@codeup.aliyun.com:renlijia/lotus/tauri-pilot.git
```

- 报 `Permission denied (publickey)` → 你的 ssh key 没在云效注册，去云效 Profile → SSH 公钥 添加
- 报 `repository not found` → 你的账号没仓权限，找 pzc 加

### Q5. `tauri-pilot aijia screenshot` 30 秒超时

老版本 plugin 没有 `skipFonts:true` 默认。需要让 cargo 重新拉云效 tauri-pilot 仓的最新 main：

```bash
# 在 lotus-app 这边
cd ~/IdeaProjects/lotus-app/src-tauri
cargo update -p tauri-plugin-pilot   # 强制重新拉 git dep 最新 commit
# 然后重启 dev server
pkill -f 'target/debug/aijia'
rm -f /tmp/tauri-pilot-com.aijia.app.sock
pnpm dev:with-pilot
```

### Q6. 我想改 aijia 子命令 / 加新命令

不建议直接改 `~/.cargo/git/` 缓存（cargo 会覆盖）。正经流程：

1. **本地 clone** tauri-pilot 到任意目录 + 改代码：
   ```bash
   git clone git@codeup.aliyun.com:renlijia/lotus/tauri-pilot.git ~/workspace/tauri-pilot
   ```
2. **临时让 lotus-app 走本地路径**：在 `~/.cargo/config.toml` 加：
   ```toml
   [patch."ssh://git@codeup.aliyun.com/renlijia/lotus/tauri-pilot.git"]
   tauri-plugin-pilot = { path = "/Users/<you>/workspace/tauri-pilot/crates/tauri-plugin-pilot" }
   ```
   这样 cargo 解析 git dep 时会用你的本地 patch 路径。改完即时生效，不用 push。
3. **改完测试 OK 后**：在 tauri-pilot 仓 commit + push 到云效 main：
   ```bash
   cd ~/workspace/tauri-pilot && git push origin main
   ```
4. **其他人**下次 `cargo update -p tauri-plugin-pilot` 就拿到了。
5. **撤销本地 patch**：把 `~/.cargo/config.toml` 里那段 `[patch.*]` 删掉。

主要文件：

- `crates/tauri-pilot-cli/src/cli.rs` — clap 定义
- `crates/tauri-pilot-cli/src/aijia.rs` — 16 个命令实现 + 共享 helper
- `crates/tauri-plugin-pilot/js/bridge.js` — 注入 webview 的 JS

CLI binary 想更新（你或别人推了新 commit 后），重跑第三步的 `cargo install` 命令带 `--force` 即可。

完整的 "添加新 aijia 子命令" 流程见 tauri-pilot 仓 `SKILL.md` 的 "Adding a new aijia subcommand" 段。

---

## 反馈

跑通后或卡住时找 pzc，或在团队群里 @ 一下。
