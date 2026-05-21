# E2E 工具链 dual-manifest 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 `tauri-plugin-pilot` 从主仓 `src-tauri/Cargo.toml` / `Cargo.lock` 中彻底移除，迁移到 `src-tauri/.e2e/` wrapper crate，使没 codeup 权限的同事和 GitHub CI 都能正常 build。

**Architecture:** Dual-manifest：主仓维持干净的 `Cargo.toml`（无 pilot），新增 `src-tauri/.e2e/` 子目录作为 wrapper crate（含 pilot path dep + 独立 `Cargo.lock`），通过 symlink 农场共享主仓所有源码和资产。`pnpm tauri:dev` 走主 manifest；`pnpm dev:with-pilot` 走 wrapper。

**Tech Stack:** Cargo manifest / Cargo features / Tauri 2.x CLI / Unix symlinks / pnpm scripts

**Spec:** `docs/superpowers/specs/2026-05-21-e2e-toolchain-dual-manifest-design.md`

**Pre-conditions（已就绪）：**
- Prototype 已实测通过：主线 `cargo check` ✅ / `.e2e/` wrapper `cargo check --features e2e` ✅ / `CARGO_NET_OFFLINE=true` 离线模式 ✅
- 当前 git stash 中有 stash@{0}：含主 `Cargo.toml` + `Cargo.lock` 的清理改动
- 当前工作树有 `src-tauri/.e2e/` 目录（未 track，prototype 产物）
- `../tauri-pilot/` sibling clone 存在
- 当前分支：`feature/e2e-toolchain`

---

## Task 1: 应用主 manifest 清理 + 验证主线编译（同事场景）

**Files:**
- Modify: `src-tauri/Cargo.toml`（删 pilot dep + 改 e2e 为 pure feature）
- Modify: `src-tauri/Cargo.lock`（重生，去掉 pilot entry）

- [ ] **Step 1: 应用 stash 0（拿回主 manifest 清理改动）**

Run:
```bash
git stash pop stash@{0}
```

Expected output:
```
On branch feature/e2e-toolchain
Changes not staged for commit:
  modified:   src-tauri/Cargo.lock
  modified:   src-tauri/Cargo.toml
```

如果 stash 已应用过（pop 报错），跳过本步。

- [ ] **Step 2: 校验 `src-tauri/Cargo.toml` 已删除 pilot dep**

Run:
```bash
grep -c "tauri-plugin-pilot" src-tauri/Cargo.toml
```

Expected output: `0`

- [ ] **Step 3: 校验 `src-tauri/Cargo.toml` 中 e2e feature 改为 pure**

Run:
```bash
grep -A2 '^\[features\]' src-tauri/Cargo.toml
```

Expected output 包含：
```
[features]
default = []
...
e2e = []
```

不应包含 `e2e = ["dep:tauri-plugin-pilot"]`。

- [ ] **Step 4: 校验 `src-tauri/Cargo.lock` 不含 pilot entry**

Run:
```bash
grep -c "tauri-plugin-pilot" src-tauri/Cargo.lock
```

Expected output: `0`

如果不是 0，跑：
```bash
rm src-tauri/Cargo.lock && cd src-tauri && cargo generate-lockfile
```

然后重新校验。

- [ ] **Step 5: 模拟同事场景验证主线干净版编译**

Run:
```bash
# 清缓存模拟"没接触过 pilot"
rm -rf ~/.cargo/git/checkouts/tauri-pilot-* ~/.cargo/git/db/tauri-pilot-* 2>&1
find src-tauri/target -name "*tauri_plugin_pilot*" -o -name "*tauri-plugin-pilot*" 2>/dev/null | xargs rm -rf
# 离线 + 无 cache + 无 SSH = 同事处境
cd src-tauri && CARGO_NET_OFFLINE=true cargo check 2>&1 | tail -3
```

Expected output 末尾包含：`Finished \`dev\` profile [unoptimized + debuginfo] target(s) in ...s`

不应包含任何 `failed to load source for dependency`。

---

## Task 2: 校验 `.e2e/` wrapper 完整性 + 编译验证

**Files:**
- Verify exists: `src-tauri/.e2e/Cargo.toml`
- Verify exists: `src-tauri/.e2e/tauri.conf.json`
- Verify symlinks: 11 个 symlink 指向 `../*`

- [ ] **Step 1: 校验 `.e2e/` 目录存在且包含 prototype 产物**

Run:
```bash
ls src-tauri/.e2e/Cargo.toml src-tauri/.e2e/tauri.conf.json
ls -la src-tauri/.e2e/src src-tauri/.e2e/build.rs src-tauri/.e2e/capabilities-e2e
```

Expected output: 两份文件存在；3 个 symlink 显示 `-> ../src` / `-> ../build.rs` / `-> ../capabilities-e2e`。

如果不存在（plan 在干净状态执行），跳到 Step 2-3 重建；否则直接到 Step 4。

- [ ] **Step 2: （如需重建）创建 `.e2e/` 目录 + symlink 农场**

Run:
```bash
mkdir -p src-tauri/.e2e
cd src-tauri/.e2e
for link in src build.rs capabilities capabilities-e2e icons prompts resources tests gen Info.plist python-runtime requirements.txt; do
  ln -sf "../$link" "./$link"
done
ls -la
cd /Users/a20250311/IdeaProjects/lotus-app
```

Expected output: 12 个 symlink 全部建立（注：实际是 12 个，spec 中"11 个"为约数）。

- [ ] **Step 3: （如需重建）写 `.e2e/Cargo.toml`**

Content（完整副本 + pilot path dep + `[lib] path = "src/lib.rs"` 因为 src 是 symlink）：

参考 src-tauri/Cargo.toml 完整复制，diff 仅以下：
- 删去 `[lib] crate-type = ...` 上面的 `name = "app_lib"` 行**前**加 `path = "src/lib.rs"`
- 在 deps 段加：`tauri-plugin-pilot = { path = "../../../tauri-pilot/crates/tauri-plugin-pilot", optional = true }`
- 把 `[features] e2e = []` 改为 `e2e = ["dep:tauri-plugin-pilot"]`

如 prototype 已在工作树，跳过此步。

- [ ] **Step 4: 校验 `.e2e/Cargo.toml` 含 pilot path dep**

Run:
```bash
grep "tauri-plugin-pilot" src-tauri/.e2e/Cargo.toml
```

Expected output:
```
tauri-plugin-pilot = { path = "../../../tauri-pilot/crates/tauri-plugin-pilot", optional = true }
```

- [ ] **Step 5: 校验 `.e2e/tauri.conf.json` 的 frontendDist 相对路径已加层**

Run:
```bash
grep "frontendDist" src-tauri/.e2e/tauri.conf.json
```

Expected output 包含 `"frontendDist": "../../dist"`。

- [ ] **Step 6: 编译验证 `.e2e/` wrapper（你 e2e 场景）**

Run:
```bash
cargo check --manifest-path src-tauri/.e2e/Cargo.toml --features e2e 2>&1 | tail -3
```

Expected output 末尾包含：`Finished \`dev\` profile [unoptimized + debuginfo] target(s) in ...s`

- [ ] **Step 7: 校验两份 lockfile 互不污染**

Run:
```bash
echo "主 Cargo.lock pilot 数: $(grep -c "tauri-plugin-pilot" src-tauri/Cargo.lock)"
echo ".e2e Cargo.lock pilot 数: $(grep -c "tauri-plugin-pilot" src-tauri/.e2e/Cargo.lock)"
```

Expected output:
```
主 Cargo.lock pilot 数: 0
.e2e Cargo.lock pilot 数: 2
```

---

## Task 3: 配套文件（.gitignore / package.json / docs）

**Files:**
- Modify: `src-tauri/.gitignore`（加 `/.e2e/target/`）
- Modify: `package.json`（改 `dev:with-pilot` 脚本）
- Create: `docs/onboarding-e2e.md`

- [ ] **Step 1: 加 `.e2e/target/` 到 src-tauri/.gitignore**

注意：`.e2e/Cargo.lock` **不** gitignore——所有 e2e 同事共享同一份 lockfile 保证可复现 build。只忽略 target/ 编译产物。

Run:
```bash
cat >> src-tauri/.gitignore <<'EOF'

# E2E wrapper crate target dir (independent from main /target/)
/.e2e/target/
EOF
```

- [ ] **Step 2: 校验 .gitignore 已更新**

Run:
```bash
tail -3 src-tauri/.gitignore
```

Expected output 包含：
```
# E2E wrapper crate target dir (independent from main /target/)
/.e2e/target/
```

- [ ] **Step 3: 改 package.json 的 dev:with-pilot 命令**

Edit `package.json`，找到现有 `"dev:with-pilot": "tauri dev --features e2e"`，改为：

```json
"dev:with-pilot": "cd src-tauri/.e2e && tauri dev --features e2e"
```

`predev:with-pilot` 不动，保持 `pnpm ensure:e2e-prereq && pnpm ensure:runtime`。

- [ ] **Step 4: 校验 package.json**

Run:
```bash
grep "dev:with-pilot" package.json
```

Expected output 包含：
```
"dev:with-pilot": "cd src-tauri/.e2e && tauri dev --features e2e"
```

- [ ] **Step 5: 写 docs/onboarding-e2e.md**

Create file `docs/onboarding-e2e.md` with this content:

```markdown
# E2E 工具链上手指南

本文给**要写 / 跑意图测试的同事**看。普通业务开发不需要读本文——`pnpm tauri:dev` 直接跑即可，零额外配置。

## 谁需要读

| 你的角色 | 需要读吗？ |
|---|---|
| 写业务代码（前端/后端） | ❌ 不需要，跳过 |
| 写 / 跑意图测试（test-intents） | ✅ 需要 |
| 维护 tauri-pilot 本身 | ✅ 需要 |

## 前置条件

- macOS（pilot 工具链当前 macOS-only，Windows 不支持）
- 仁励家 codeup 账号 + SSH key 已配
- 申请 `codeup.aliyun.com/renlijia/lotus/tauri-pilot` read 权限

## 一次性 Setup

```bash
# 1. clone pilot 到 lotus-app 同级目录
cd /your/IdeaProjects     # 跟 lotus-app 同级
git clone ssh://git@codeup.aliyun.com/renlijia/lotus/tauri-pilot.git

# 验证目录布局：
# IdeaProjects/
# ├── lotus-app/
# └── tauri-pilot/
#     └── crates/tauri-plugin-pilot/
```

## 跑 e2e

```bash
cd lotus-app
pnpm dev:with-pilot       # 内部跑 cd src-tauri/.e2e && tauri dev --features e2e
```

跑起来后，意图测试 runner 可通过 `tauri-pilot` 提供的 IPC 端口跟 dev server 通信。

详见 `.claude/skills/test-intents-runner/SKILL.md`。

## 架构原理

为什么是 `cd src-tauri/.e2e && tauri dev`？

主仓 `src-tauri/Cargo.toml` 不含 `tauri-plugin-pilot` dep——让没 pilot 仓权限的同事和 CI 都能正常 build。e2e 单独有个 wrapper crate 在 `src-tauri/.e2e/`，含 pilot path dep + 独立 `Cargo.lock`，通过 symlink 共享主仓所有源码。

详见 `docs/superpowers/specs/2026-05-21-e2e-toolchain-dual-manifest-design.md`。

## 常见问题

**Q: 报错 `path source '../../../tauri-pilot/...' does not exist`**

A: 你没 clone pilot 到 lotus-app 同级目录。回到上面"一次性 Setup"。

**Q: `pnpm tauri:dev` 跟 `pnpm dev:with-pilot` 有什么区别？**

A: `tauri:dev` 走主 manifest（无 pilot），日常业务开发用。`dev:with-pilot` 走 wrapper manifest（含 pilot），跑意图测试时用。

**Q: 我加了个新 dep 到主 `Cargo.toml`，e2e build 编译失败说缺 dep**

A: `.e2e/Cargo.toml` 是手动维护的主 Cargo.toml 副本。把你新加的 dep 行也复制到 `.e2e/Cargo.toml` 同位置即可。

**Q: Windows 能跑 e2e 吗？**

A: 不能。pilot 工具链当前 macOS-only。Windows 同事跑业务开发不受影响。
```

- [ ] **Step 6: 校验 docs/onboarding-e2e.md 已创建**

Run:
```bash
ls -la docs/onboarding-e2e.md && wc -l docs/onboarding-e2e.md
```

Expected output: 文件存在，~80 行。

---

## Task 4: 端到端实测（主线 dev + e2e dev）

**Files:**（无改动，纯验证）

- [ ] **Step 1: 实跑 `pnpm tauri:dev` 验证主线 dev server 启动**

Run (in background terminal 1):
```bash
pnpm tauri:dev
```

Expected: 看到 Vite dev server 起在 `http://127.0.0.1:5173`，几分钟后 Tauri 窗口弹出，AIjia 主界面渲染。

验证：手动操作 UI 5-10 秒，确认基本功能（侧栏点击、进入设置等）正常。

终止：Ctrl+C 在 terminal 1。

- [ ] **Step 2: 实跑 `pnpm dev:with-pilot` 验证 e2e dev server**

前置：确认 `../tauri-pilot/` sibling clone 存在：
```bash
ls -d ../tauri-pilot/crates/tauri-plugin-pilot
```

Run (in background terminal 1):
```bash
pnpm dev:with-pilot
```

Expected:
- Vite 起 (同 Step 1)
- cargo 编译 wrapper crate（首次 ~2min，含 pilot）
- Tauri 窗口弹出，AIjia 主界面渲染
- 日志中应看到 `tauri-plugin-pilot` 注册痕迹（或不报错即可）

终止：Ctrl+C 在 terminal 1。

- [ ] **Step 3: （可选）跑一遍现有意图测试**

如果时间允许，挑一个简单的意图测试验证 pilot 通道：
```bash
# 例如登录意图（参考 .claude/skills/test-intents-runner/SKILL.md）
pnpm exec aijia ...
```

不强制要求，但能跑通最稳。

---

## Task 5: 回归测试

**Files:**（无改动，纯验证）

- [ ] **Step 1: 前端 vitest**

Run:
```bash
pnpm test 2>&1 | tail -10
```

Expected output 末尾包含：`Test Files <N> passed (N)` 或类似全过提示。

- [ ] **Step 2: Rust 关键集成测试**

Run:
```bash
cd src-tauri && cargo test --test tauri_event_adapter_test -- --nocapture 2>&1 | tail -10
```

Expected output 末尾包含：`test result: ok. <N> passed; 0 failed`

- [ ] **Step 3: Rust review_ 系列回归（验证架构约束）**

Run:
```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -15
```

Expected: 所有 review_ 测试通过或维持原状态（如有 pre-existing failures 跟本 PR 无关，可忽略）。

---

## Task 6: Cleanup commit + push

**Files:** Stage 所有改动

- [ ] **Step 1: 看清要 stage 的内容**

Run:
```bash
git status --short
```

Expected output 包含：
```
 M src-tauri/Cargo.toml
 M src-tauri/Cargo.lock
 M src-tauri/.gitignore
 M package.json
?? src-tauri/.e2e/Cargo.toml
?? src-tauri/.e2e/Cargo.lock
?? src-tauri/.e2e/tauri.conf.json
?? src-tauri/.e2e/<11-12 个 symlink>
?? docs/onboarding-e2e.md
```

确认**没有**意外文件（例如 `src-tauri/.e2e/target/` 不应出现，被 .gitignore 拦下）。

- [ ] **Step 2: Stage 改动**

Run:
```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/.gitignore package.json
git add src-tauri/.e2e/
git add docs/onboarding-e2e.md
```

注意：**不要** `git add .` 或 `git add -A`，避免误带 23 个未相关的 modified frontend 文件进 commit。

- [ ] **Step 3: 校验 staged 内容**

Run:
```bash
git status --short | grep -v "^M\|^A" | head -30   # 看 staged
git diff --staged --stat
```

Expected: staged 文件清单和 Step 1 列表一致。

- [ ] **Step 4: 创建 cleanup commit**

Run:
```bash
git commit -m "$(cat <<'COMMIT'
chore(e2e): switch to dual-manifest, remove pilot from main Cargo.toml

Main src-tauri/Cargo.toml no longer references tauri-plugin-pilot at all.
The pilot dep, lockfile entry, and registration logic now live in a
wrapper crate at src-tauri/.e2e/ which shares source via a symlink farm.

Contributors without codeup access (and GitHub CI) can build normally:
their pnpm tauri:dev hits the clean main manifest. E2E work uses
pnpm dev:with-pilot which targets the wrapper.

Changes:
- src-tauri/Cargo.toml: remove tauri-plugin-pilot dep, change e2e
  feature from ["dep:tauri-plugin-pilot"] to [] (pure feature)
- src-tauri/Cargo.lock: regenerate without pilot entries
- src-tauri/.gitignore: add /.e2e/target/
- src-tauri/.e2e/: new wrapper crate (Cargo.toml + tauri.conf.json
  + Cargo.lock + symlink farm to ../src, ../build.rs, ../capabilities,
  etc.)
- package.json: dev:with-pilot → cd src-tauri/.e2e && tauri dev --features e2e
- docs/onboarding-e2e.md: new type-B contributor guide

Spec: docs/superpowers/specs/2026-05-21-e2e-toolchain-dual-manifest-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
COMMIT
)"
```

- [ ] **Step 5: 校验 commit 成功 + 状态干净**

Run:
```bash
git log --oneline -1
git status --short | head -10
```

Expected:
- 第一行：新 commit hash + "chore(e2e): switch to dual-manifest..."
- 状态：只剩 23 个未相关的 frontend 工作树改动，无新增 staged / untracked

- [ ] **Step 6: （手动决策）push 与否**

Plan 不自动 push。决定 push 与否由用户判断：
- 如果意图先本地多测一阵 → 不 push
- 如果意图发起 PR review → `git push origin feature/e2e-toolchain`

---

## 完成后检查清单

- [ ] 主 `Cargo.toml` 不含 `tauri-plugin-pilot`
- [ ] 主 `Cargo.lock` 不含 pilot entry
- [ ] `.e2e/Cargo.toml` 含 pilot path dep
- [ ] `.e2e/Cargo.lock` 含 pilot entry
- [ ] `pnpm tauri:dev` 跑通（业务场景）
- [ ] `pnpm dev:with-pilot` 跑通（e2e 场景）
- [ ] 离线 + 无 cache 时主线 `cargo check` 跑通（同事场景）
- [ ] 回归测试全过（vitest + cargo test）
- [ ] cleanup commit 已创建
- [ ] `docs/onboarding-e2e.md` 存在
- [ ] 23 个未相关 frontend 改动**没**混进 cleanup commit
