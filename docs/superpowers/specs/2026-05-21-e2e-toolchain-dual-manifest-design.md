# E2E 工具链 dual-manifest 设计

**日期**：2026-05-21
**作者**：pzc + Claude
**状态**：草案 → 待用户 review
**关联**：feature/e2e-toolchain 分支收尾

---

## 背景与问题

`feature/e2e-toolchain` 分支为接入 `tauri-plugin-pilot`（仁励家私有云效仓的 e2e 自动化插件）做了以下改动：

```toml
# src-tauri/Cargo.toml
tauri-plugin-pilot = { git = "ssh://git@codeup.aliyun.com/renlijia/lotus/tauri-pilot.git", branch = "main", optional = true }

[features]
e2e = ["dep:tauri-plugin-pilot"]
```

按设计意图，`optional = true` + `#[cfg(feature = "e2e")]` gate 应该让没传 `--features e2e` 的默认 build 不感知 pilot。但实际：

**复现现象**：
- 清空本地 `~/.cargo/git/checkouts/tauri-pilot-*` cache
- `CARGO_NET_OFFLINE=true cargo check --no-default-features` 失败，报：
  ```
  unable to update ssh://git@codeup.aliyun.com/renlijia/lotus/tauri-pilot.git
  can't checkout from ...: you are in the offline mode
  ```

**根因**：cargo 在解析 `Cargo.lock` 时会校验**所有** `[[package]]` entry 的 source 可达，即便该 dep 在当前 feature set 下没被激活。lockfile 第 5448 行写了 `source = "git+ssh://...codeup..."`，cargo 必须 fetch 验证 → 没 SSH 权限的同事和 GitHub CI runner 必挂。

**铁律**：
> 任何写在 Cargo.toml 的 dep（不论 `optional` / `target` / `feature` 怎么 gate），cargo 都会写进 Cargo.lock。
> 任何 Cargo.lock 里的 git ssh source，cargo 解析 lockfile 时都会去 fetch——`--no-default-features` / `--features off` 都救不了。

这是 cargo 的硬性行为，无配置开关可绕过。结论：**主仓 `Cargo.toml` 里有 pilot 那一行 → 同事 + CI 必挂**。

## 目标

| # | 目标 |
|---|---|
| 1 | 没 codeup 权限的同事 `git pull main && pnpm tauri:dev` 直接通 |
| 2 | GitHub CI（Windows release runner）`cargo build --release` 直接通 |
| 3 | 你 / e2e 同事本地 `pnpm dev:with-pilot` 能跑 e2e |
| 4 | e2e 工具链（包括 build.rs / lib.rs cfg / capability 文件 / scripts / docs）合 main 共享 |
| 5 | 不公开 pilot 代码到公网 git / crates.io |

## 非目标

- 让 cargo workflow 出现"e2e mode 切换"概念
- 支持 Windows 同事跑 e2e（pilot 工具链当前已是 macOS-only，由 [2026-05-16 选型决定](../../../CLAUDE.md) 记录）

## 候选方案对比

| 方案 | 同事感受 | 你的体验 | release 包 | 工程量 | 评估 |
|---|---|---|---|---|---|
| A. e2e:on/off 脚本切换主 manifest | 完全无感 | 每次切换跑 on/off | ✅ | 中 | ❌ 心智负担：要记得 off |
| B. 双 manifest（本方案） | 完全无感 | 不切换，主 manifest 永远干净 | ✅ | 中 | ✅ 选定 |
| C. pilot 镜像到公网 git | 无感 | 不改 | ✅ | 小 | ❌ 代码暴露 |
| D. pilot 发到 cargo private registry | 无感 | 不改 | ✅ | 大 | ❌ 基础设施重 |

## 选定方案：B 双 manifest

### 核心机制

主仓 `src-tauri/Cargo.toml` 永远不含 `tauri-plugin-pilot`，对应 `Cargo.lock` 自然也没有。e2e 单独有一份 wrapper manifest 在 `src-tauri/.e2e/Cargo.toml`，含 pilot path dep + 独立的 `Cargo.lock`。两个 manifest 通过 symlink 共享所有源码和资产。

### 目录结构

```
lotus-app/
├── ../tauri-pilot/                          ← sibling clone（e2e 用户自备）
│   └── crates/tauri-plugin-pilot/
│
├── src-tauri/
│   ├── Cargo.toml                           ← 主线，无 pilot
│   ├── Cargo.lock                           ← 主线 lockfile，无 pilot entry
│   ├── tauri.conf.json                      ← 主线配置
│   ├── build.rs
│   ├── src/                                 ← 共用
│   ├── capabilities/ capabilities-e2e/      ← 共用
│   ├── icons/ prompts/ resources/ tests/    ← 共用
│   │
│   └── .e2e/                                ← e2e wrapper（git track）
│       ├── Cargo.toml                       ← 主 Cargo.toml 副本 + 加 pilot path dep
│       ├── Cargo.lock                       ← e2e 专属，含 pilot entry
│       ├── tauri.conf.json                  ← 主配置副本，相对路径加层 ../
│       ├── src                  → ../src                ← symlink
│       ├── build.rs             → ../build.rs           ← symlink
│       ├── capabilities         → ../capabilities       ← symlink
│       ├── capabilities-e2e     → ../capabilities-e2e   ← symlink
│       ├── icons                → ../icons              ← symlink
│       ├── prompts              → ../prompts            ← symlink
│       ├── resources            → ../resources          ← symlink
│       ├── tests                → ../tests              ← symlink
│       ├── gen                  → ../gen                ← symlink
│       ├── Info.plist           → ../Info.plist         ← symlink
│       ├── python-runtime       → ../python-runtime     ← symlink
│       └── requirements.txt     → ../requirements.txt   ← symlink
│
├── docs/onboarding-e2e.md                   ← 类型 B 同事入门文档
└── package.json
    ├── "tauri:dev": "tauri dev"                                                    ← 不变
    └── "dev:with-pilot": "cd src-tauri/.e2e && tauri dev --features e2e"          ← 改
```

### 工作流

| 场景 | 命令 | 走的 manifest |
|---|---|---|
| 普通同事 / 你跑业务 | `pnpm tauri:dev` | `src-tauri/Cargo.toml`（干净） |
| 你 / e2e 同事跑 e2e | `pnpm dev:with-pilot` | `src-tauri/.e2e/Cargo.toml`（含 pilot） |
| CI release | `cargo build --release` | `src-tauri/Cargo.toml`（干净） |

### Symlink 设计理由

- **不复制源码**：改 `src/lib.rs` 一次，主项目和 wrapper 都看到
- **不改源码相对路径**：`build.rs` 用 `CARGO_MANIFEST_DIR` 拼 `capabilities-e2e/pilot.json`，在 wrapper 下 `CARGO_MANIFEST_DIR = src-tauri/.e2e/`，symlink 让 `.e2e/capabilities-e2e` 透明转发到 `../capabilities-e2e` → build.rs 无需感知差异
- **唯一例外**：`.e2e/tauri.conf.json` 中 `frontendDist: "../dist"` → `"../../dist"`，因为 tauri CLI 读配置时 cwd 是 `.e2e/`，相对路径多一层

### Pilot 引用

```toml
# .e2e/Cargo.toml
tauri-plugin-pilot = { path = "../../../tauri-pilot/crates/tauri-plugin-pilot", optional = true }
```

注意：
- 不引用 pilot 仓根（那是 cargo workspace virtual manifest，不能直接 `path =`）
- 引用 workspace 内的 `tauri-plugin-pilot` crate
- 用 `path` 而非 `git` —— 即便误推到 main，类型 A 同事看到的报错是「本地路径不存在」（温和、可自救），而非「SSH 鉴权失败」（涉及权限申请）

### Windows 兼容

- macOS / Linux：symlink 原生支持
- Windows：git pull 默认把 symlink 解码成普通文本文件（除非 `core.symlinks=true`）。Windows 同事**不跑 e2e**（pilot 已是 macOS-only），业务开发只走主 `Cargo.toml` 不读 `.e2e/`，**完全不受影响**。Windows 同事即便想跑 e2e 也会失败——失败方式清晰：cargo 报找不到 `src/lib.rs` 等
- 这是有意 trade-off：当前不需要 Windows e2e 能力

## 改动清单

| # | 文件 | 改动 | 行数 |
|---|---|---|---|
| 1 | `src-tauri/Cargo.toml` | 删 pilot dep 段（16 行带注释）+ 改 `e2e = ["dep:..."]` 为 `e2e = []` pure feature | -16 / 改 1 |
| 2 | `src-tauri/Cargo.lock` | `cargo update -p aijia` 重生，自动去 pilot entry | 自动 |
| 3 | `src-tauri/.e2e/Cargo.toml` | 新建，主副本 + pilot path dep | +110 |
| 4 | `src-tauri/.e2e/tauri.conf.json` | 新建，主配置副本 + 相对路径加 `../` | +50 |
| 5 | `src-tauri/.e2e/Cargo.lock` | `cd .e2e && cargo check --features e2e` 自动生成 | 自动 |
| 6 | `src-tauri/.e2e/` 下 11 个 symlink | `ln -s` 命令 | 11 条 |
| 7 | `src-tauri/.gitignore` | 加 `/.e2e/target/` 防 wrapper target dir 被提交 | +1 |
| 8 | `package.json` | 改 `dev:with-pilot` 的命令字符串 | 改 1 |
| 9 | `docs/onboarding-e2e.md` | 新建类型 B 同事入门文档 | +60 |

**不需要改**：
- `src-tauri/src/`（任何 Rust 源码，含 `#[cfg(feature="e2e")]` gate 保持不动）
- `src-tauri/build.rs`
- `src-tauri/capabilities*/`
- `.github/workflows/`
- 21 个 frontend 业务文件
- `.claude/skills/test-intents-*`

## 误推保险（弱保险，可接受）

**不引入** pre-commit hook、sync 脚本等强保险机制。理由：

- 主 `Cargo.toml` 物理上没 pilot 行——除非你手动编辑，否则不会"误推 dirty 主 manifest"
- 即便误推（极端情况），同事看到的是 `.e2e/Cargo.toml` 的 `path = "../../../tauri-pilot/..."` 报错——温和、本地可修，不需要权限申请

## 同事 / e2e 用户 onboarding

### 类型 A：普通同事（99% 的人）

零操作，跟主线一样：
```bash
git clone <repo>
pnpm install
pnpm tauri:dev    # ✅
```

### 类型 B：要跑 e2e 的同事

```bash
# 1. 找仓 owner 开通 codeup.aliyun.com/renlijia/lotus/tauri-pilot read 权限
# 2. clone 到 lotus-app 同级目录
cd /your/IdeaProjects && git clone ssh://git@codeup.aliyun.com/renlijia/lotus/tauri-pilot.git

# 3. （macOS only 提醒）确认你不在 Windows
# 4. 跑 e2e
cd lotus-app
pnpm dev:with-pilot
```

详见 `docs/onboarding-e2e.md`。

## feature 分支收尾

**方案**：单个 cleanup commit（不重写历史）

```
chore(e2e): switch to dual-manifest, remove pilot from main Cargo.toml

- Remove tauri-plugin-pilot git dep from src-tauri/Cargo.toml
- Change e2e feature from ["dep:tauri-plugin-pilot"] to [] (pure feature)
- Regenerate src-tauri/Cargo.lock without pilot entry
- Add src-tauri/.e2e/{Cargo.toml, tauri.conf.json, Cargo.lock, symlinks}
- Update package.json: dev:with-pilot → cd .e2e && tauri dev --features e2e
- Add docs/onboarding-e2e.md for type-B contributors
```

合 main 时主 `Cargo.lock` 经历"曾有 pilot → 后清掉"的演进——cosmetic issue，无数据问题。

## 测试计划

### Prototype 已通过验证（2026-05-21 实测）

- ✅ `.e2e/` wrapper `cargo check --features e2e` 编译过（1m49s）
- ✅ 主 `Cargo.toml` 删 pilot 行后 `cargo check` 编译过
- ✅ 离线模式 + 无 cache（同事场景）`CARGO_NET_OFFLINE=true cargo check` 编译过
- ✅ 两份 `Cargo.lock` 完全独立，互不污染

### Plan 阶段补验证

- [ ] `pnpm tauri:dev` 实际起 dev server（不只是 cargo check）
- [ ] `pnpm dev:with-pilot` 实际起 dev server + pilot 插件正常注册
- [ ] 跑一遍现有意图测试，确认 e2e CLI 端到��工作流不挂

### 回归测试

- [ ] `pnpm test`（前端 vitest）
- [ ] `cd src-tauri && cargo test`（Rust 单测 + 集成测试）

## 决策记录

| 决策 | 选项 | 理由 |
|---|---|---|
| 同步主 / wrapper manifest | 手动维护，无 sync 脚本 | 主 Cargo.toml deps 变更频率低（~月一次）；脚本维护成本 > 手动同步 |
| 防误推保险 | 不要 pre-commit hook | 主 manifest 物理上没 pilot 行，不会"忘了 off"误推 |
| wrapper 位置 | `src-tauri/.e2e/` 子目录 | tauri CLI 硬要求 `Cargo.toml` 和 `tauri.conf.json` 同目录；放主仓 `src-tauri/` 同级会让 lockfile 冲突 |
| 共用源码方式 | symlink 农场 | 不复制源码、不改 build.rs / tauri.conf.json 中相对路径 |
| pilot dep 形式 | `path` 而非 `git` | 误推时报错温和，可本地修复 |
| feature 分支收尾 | cleanup commit | 不重写历史，操作风险低 |

## 风险

| 风险 | 严重性 | 缓解 |
|---|---|---|
| Windows 同事 git pull 后 symlink 变文本 | 低 | Windows 同事不跑 e2e；主线 `pnpm tauri:dev` 走 `src-tauri/Cargo.toml`，根本不读 `.e2e/` 下的 symlink，business 工作流不受影响 |
| 主 Cargo.toml 加新 dep 忘了同步到 `.e2e/Cargo.toml` | 中 | e2e build 会失败、错误清晰，e2e 同事自己同步即可，无生产影响 |
| `.e2e/target/` 被误提交（编译产物） | 低 | `.gitignore` 加 `/.e2e/target/` |
| 未来 tauri CLI 升级改变 `tauri.conf.json` 相对路径解析 | 低 | 路径调整本来就要跟 tauri 行为对齐；锁版本号缓解 |
