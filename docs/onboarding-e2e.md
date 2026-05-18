# 接入 AIjia e2e 测试：同事上手指南

> 创建：2026-05-18
> 受众：lotus-app 开发者（**本地开发**，不是 CI）
> 关联：`docs/e2e-org1-chat-mainline.md`（CLI 工具规格）、`docs/e2e-testing-decisions.md`（选型决策）

---

## 你将获得什么

跑完这份指南，你的本地环境会有：

1. 一个能用的 `tauri-pilot` 命令（在 `~/.cargo/bin/`），自带 `aijia` 子命令组
2. AIjia dev server 起来后，能用 `tauri-pilot aijia health-check` / `aijia send` / `aijia wait-reply` 等 16 个命令操作 webview

预计耗时：**首次 15–20 分钟**（含 Rust 工具链下载、tauri-pilot 编译）。日常使用零启动开销。

---

## 前置依赖

| 工具 | 推荐版本 | 检测命令 |
|---|---|---|
| macOS | 任意 | `sw_vers` |
| Rust | ≥ 1.95.0 | `rustc --version`（不够就 `rustup install 1.95.0`） |
| pnpm | ≥ 8 | `pnpm --version`（已有的 AIjia 开发环境就够了） |
| Tauri 2 系统依赖 | 标准 | `pnpm tauri:dev` 能跑就 OK |

---

## 第一步：克隆两个仓到同级目录

**硬约束**：lotus-app 和 tauri-pilot 必须是**同一父目录下的两个 sibling**。`lotus-app/src-tauri/Cargo.toml` 用相对路径 `../../tauri-pilot/crates/tauri-plugin-pilot` 解析 plugin 源码，目录布局错了立刻挂。

推荐布局：

```
~/IdeaProjects/                          (或任何你喜欢的父目录)
├── lotus-app/                           ← AIjia 主仓
└── tauri-pilot/                         ← e2e CLI fork（仓库地址见下）
```

操作：

```bash
mkdir -p ~/IdeaProjects && cd ~/IdeaProjects
git clone <lotus-app 仓库地址>           # 你应该已经有
git clone <tauri-pilot fork 仓库地址>    # ← 重点
```

> tauri-pilot 仓库地址：内部分发，找 pzc 拿。**不是**上游 `mpiton/tauri-pilot`——上游没有 `aijia` 子命令组。

---

## 第二步：装 tauri-pilot CLI

进入 tauri-pilot 仓，用 `cargo install` 把 CLI binary 装到 `~/.cargo/bin/`：

```bash
cd ~/IdeaProjects/tauri-pilot
cargo install --path crates/tauri-pilot-cli --bin tauri-pilot --force
```

- 首次大约 5–8 分钟（拉依赖 + release 编译）
- 装完后 `which tauri-pilot` 应返回 `~/.cargo/bin/tauri-pilot`
- `tauri-pilot --version` 应输出 `tauri-pilot-cli 0.5.2`
- `tauri-pilot aijia --help` 应列出 16 个子命令；如果只显示通用命令没有 `aijia`，说明拉的是上游而不是 fork，回第一步重检

后续 tauri-pilot 仓更新时（同事改了 aijia 子命令、修了 bug 等）：

```bash
cd ~/IdeaProjects/tauri-pilot && git pull
cargo install --path crates/tauri-pilot-cli --bin tauri-pilot --force
```

`--force` 是必要的，否则 cargo 看版本号没变就跳过安装。

---

## 第三步：启动 AIjia dev server

```bash
cd ~/IdeaProjects/lotus-app
pnpm install                    # 如果之前没装过
pnpm tauri:dev
```

首次跑会触发完整 Rust 编译（含 tauri-plugin-pilot），大约 5–10 分钟。

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

## 第四步：常用命令速查

完整 16 个命令文档：`<tauri-pilot 仓>/SKILL.md` 的 "App-specific subcommand groups" 章节。

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

### Q4. AIjia 编译失败：`could not find tauri-plugin-pilot at ../../tauri-pilot/...`

目录布局错了。`lotus-app/` 和 `tauri-pilot/` 必须是 sibling：

```bash
ls -d ../../tauri-pilot   # 站在 lotus-app/ 里跑，应该列出 tauri-pilot 仓
```

如果不在，把 tauri-pilot mv 到 lotus-app 的父目录里。

### Q5. `tauri-pilot aijia screenshot` 30 秒超时

老版本 plugin 没有 `skipFonts:true` 默认。`cd ~/IdeaProjects/tauri-pilot && git pull` 拉到含 `fix(plugin): default screenshot to skipFonts ...` 这条 commit 的版本，然后**回 lotus-app 重启 dev server**（plugin 改了要重编）：

```bash
# lotus-app 那边
touch src-tauri/Cargo.toml    # 强制 cargo 重新看 plugin
pkill -f 'target/debug/aijia'  # 杀掉旧 AIjia
rm -f /tmp/tauri-pilot-com.aijia.app.sock
pnpm tauri:dev
```

### Q6. 我想改 aijia 子命令 / 加新命令

正合适。tauri-pilot 仓是本地 fork，主要文件：

- `crates/tauri-pilot-cli/src/cli.rs` — clap 定义
- `crates/tauri-pilot-cli/src/aijia.rs` — 16 个命令实现 + 共享 helper
- `crates/tauri-plugin-pilot/js/bridge.js` — 注入 webview 的 JS，含 `screenshot` 等底层 RPC

改完跑 `cargo install --path crates/tauri-pilot-cli --bin tauri-pilot --force` 就生效。如果改了 plugin（bridge.js / handler.rs / lib.rs），需要在 lotus-app 那边重启 dev server。

完整的 "添加新 aijia 子命令" 流程见 tauri-pilot 仓 `SKILL.md` 的 "Adding a new aijia subcommand" 段。

---

## 反馈

跑通后或卡住时找 pzc，或在团队群里 @ 一下。
