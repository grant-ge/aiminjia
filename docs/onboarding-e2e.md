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

## 第一步：申请 tauri-pilot 仓权限

tauri-pilot fork 仓在云效私库 **`git@codeup.aliyun.com:renlijia/lotus/tauri-pilot.git`**，找 pzc 加入。

权限的两条路径独立：

| 用途 | 所需权限 | 失败时的表现 |
|---|---|---|
| `pnpm dev:with-pilot`（lotus-app 编译时拉 plugin 源码） | 云效仓**读**权限 + 本地 SSH key 在云效注册 | `cargo fetch --features e2e` 失败 "authentication failed" |
| `tauri-pilot` CLI（操作 webview） | 同上 | `cargo install` 时 git clone 失败 |

> **不是**上游 `mpiton/tauri-pilot`——上游没有 `aijia` 子命令组。

测试连通性：

```bash
git ls-remote git@codeup.aliyun.com:renlijia/lotus/tauri-pilot.git
# 期望：列出 main 分支 sha；如果 fail 看本地 ssh key 是否在云效绑定
```

---

## 第二步：让 cargo 用系统 git CLI

Cargo 自带的 libgit2 不支持 ed25519 / macOS keychain 加密的 ssh key，需要让它走系统 git（git 走 ssh-agent 没问题）。

```bash
mkdir -p ~/.cargo
cat >> ~/.cargo/config.toml << 'EOF'
[net]
git-fetch-with-cli = true
EOF
```

这是全局配置，对所有 cargo 项目生效。没有这个，`cargo fetch` 会报 "no authentication methods succeeded"。

---

## 第三步：装 tauri-pilot CLI

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

---

## 第四步：启动 AIjia 带 e2e 模式

普通业务开发用 `pnpm tauri:dev`（不会触发拉 tauri-pilot）。要跑 e2e 用：

```bash
cd ~/IdeaProjects/lotus-app
pnpm install                    # 如果之前没装过
pnpm dev:with-pilot
```

`dev:with-pilot` 实际跑的是 `tauri dev --features e2e`，会：
1. 触发 build.rs 复制 `capabilities/pilot.json`
2. cargo 从云效拉 tauri-plugin-pilot 源码（首次 ~30s，之后走 ~/.cargo/git/ 缓存）
3. plugin 编进来 + lib.rs 注册（feature gate）

首次跑会触发完整 Rust 编译，大约 5–10 分钟。

启动后，新开终端跑：

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

## 第五步：常用命令速查

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

Cargo 的 libgit2 不识别你的 ssh key。回第二步配 `~/.cargo/config.toml` 加 `git-fetch-with-cli = true`。

如果加了还失败：

```bash
# 测试 git 能不能直接拉
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
