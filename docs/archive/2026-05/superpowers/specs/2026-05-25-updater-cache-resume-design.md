# Updater 跨启动缓存 + 断点续传 + 自动/手动模式

## Context

当前 updater 使用 Tauri updater plugin 的 `Update.download()` 方法下载更新包，存在三个问题：

1. **包大（74.5 MB）下载慢**——OSS 直连无 CDN，约 375 KB/s
2. **不能跨启动复用**——`Update` 句柄是内存对象，刷新/重启必丢失，每次都重新下载完整包
3. **不能断点续传**——网络中断只能从头下载，没有重试机制

同时，启动后立即自动下载会消耗用户带宽，需要给用户控制权。

## 目标

- **断点续传**：基于 HTTP Range header，从中断处续传
- **跨启动缓存**：完整包持久化，下次启动检测到直接 install 不重下
- **自动/手动模式**：设置项控制，默认自动；手动模式下显示 release notes 等用户确认
- **网络波动重试**：自动重试 3 次（2s/8s/30s 指数退避），失败后用户手动重试

## 架构概述

**Rust 后端**主导下载，前端只做 UI 和状态展示。下载完成后把字节传给 Tauri updater plugin 的 `Update::install(bytes)` API，复用 Tauri 内置的 Ed25519 签名验证（不需要自己实现 minisign）。

```
┌─────────────────────────────────────────────────────────────┐
│ 前端 (updaterStore.ts)                                       │
│  ├─ check() → 取得 Update 句柄                              │
│  ├─ invoke('updater_check_cache') → 决定 phase              │
│  ├─ invoke('updater_download') → 启动下载（自动重试 + 续传）│
│  ├─ 订阅 'updater:download-progress' 事件                   │
│  └─ install(bytes) → relaunch                               │
└─────────────────────────────────────────────────────────────┘
                            ↕
┌─────────────────────────────────────────────────────────────┐
│ Rust 后端 (src-tauri/src/updater/)                          │
│  ├─ cache.rs    — meta.json 读写、缓存目录管理               │
│  ├─ downloader.rs — reqwest + Range + 重试逻辑              │
│  └─ commands.rs — 5 个 Tauri commands                       │
│                                                              │
│ 缓存位置：~/.renlijia/global/updater/                        │
│  ├─ meta.json                                               │
│  └─ {version}.tar.gz                                        │
└─────────────────────────────────────────────────────────────┘
```

## 状态机

```
idle → checking → available* → downloading → ready → installing → idle
                       ↑           ↓
                       └─ failed ──┘
```

`available` 仅在**手动模式**下出现。**自动模式下从 `checking` 直接进 `downloading`**。

| Phase | 含义 | 右上角显示 | 弹窗内容 |
|---|---|---|---|
| `idle` | 无更新 | 无 | 关闭 |
| `checking` | 正在检查 | 无 | 关闭 |
| `available` | 有新版本（手动模式） | "v? 可用" | release notes + "立即下载" 按钮 |
| `downloading` | 下载中 | "v? 下载中 45%" | 进度条 + release notes |
| `ready` | 下载完成 | "v? 可安装" | "立即安装并重启" 按钮 + release notes |
| `failed` | 失败 | "更新失败，点击重试" | 错误信息 + "重试" 按钮 |
| `installing` | 安装中 | 无 | loading |

## 缓存设计

**目录**：`~/.renlijia/global/updater/`（不在 Tauri ACL scope 内，由 Rust 端管理）

**文件**：
```
~/.renlijia/global/updater/
├── meta.json            # 缓存元数据
└── {version}.tar.gz     # 完整或部分下载的包
```

**meta.json**：
```json
{
  "version": "0.5.29",
  "url": "https://lotus.renlijia.com/aijia/v0.5.29/AIjia.app.tar.gz",
  "expected_size": 74566666,
  "downloaded_size": 32500000,
  "complete": false,
  "etag": "9D42060DCBFC535FF87D6B2092A2B68E-15"
}
```

**版本切换处理**：服务器返回的版本号与 meta.json 不一致 → 删除整个 `updater/` 目录重来。ETag 不一致同样处理（防止同版本号但服务器包变了的边缘情况）。

## Rust 后端

### 新增模块结构

```
src-tauri/src/updater/
├── mod.rs                 # pub use
├── cache.rs               # 缓存目录管理 + meta.json 读写
├── downloader.rs          # reqwest + Range + 重试
└── commands.rs            # Tauri commands
```

### AiJiaHome 新增方法

`src-tauri/src/storage/aijia_home.rs`：

```rust
pub fn global_updater_dir(&self) -> PathBuf {
    self.global_dir().join("updater")
}
```

### Tauri commands

| Command | 输入 | 输出 | 作用 |
|---|---|---|---|
| `updater_check_cache(version, expected_size)` | 版本号、期望大小 | `{ status: 'complete' \| 'partial' \| 'none', downloaded_size }` | 启动时检测本地缓存 |
| `updater_download(url, version, expected_size, etag)` | 下载参数 | 流式进度事件 | 启动下载（支持 Range 续传） |
| `updater_cancel_download()` | - | - | 取消下载（用户关闭弹窗时不调用，仅显式取消） |
| `updater_read_cached_bytes(version)` | 版本号 | `Vec<u8>` | 读完整包字节，给 `Update::install()` |
| `updater_clear_cache()` | - | - | 清空缓存 |

### 进度事件

Rust 端通过 `app.emit()` 发送：

| Event | Payload | 时机 |
|---|---|---|
| `updater:download-progress` | `{ version, downloaded, total }` | 每接收一块数据 |
| `updater:download-failed` | `{ version, error, retried }` | 3 次重试后仍失败 |

### 重试策略

- 退避：2s → 8s → 30s（指数）
- 重试条件：网络错误（timeout/connection reset/HTTP 5xx）
- 不重试：HTTP 4xx、用户取消、磁盘满
- 3 次失败后：emit `updater:download-failed`，停留 `.part` 文件等待手动重试

### 完整性保证

- 写文件用临时 `.part`，下载完原子 rename 为最终文件
- 检查文件大小匹配 `expected_size` 才标记 `complete=true`
- 验签由 Tauri `Update::install(bytes)` 自动做（Ed25519 / minisign）
- 验签失败 → 删除缓存 + `failed` 状态（不重试，可能是攻击）

## 前端 store 改造

### 新增 state

```typescript
interface UpdaterState {
  phase: Phase  // 加 'available' 回来
  version: string | null
  notes: string
  progress: { downloaded: number; total: number } | null
  error: string | null
  autoDownload: boolean              // ← 新增：自动下载开关
  panelOpen: boolean
  online: boolean
  _update: Update | null
  _cachedBytes: Uint8Array | null    // ← 新增：完整包字节
  _bootstrapPromise: Promise<void> | null

  bootstrap(): Promise<void>
  startDownload(): Promise<void>     // 手动模式下用户点击触发
  retryDownload(): Promise<void>     // failed 状态下手动重试
  openPanel(): void
  closePanel(): void
  installNow(): Promise<void>
  setAutoDownload(value: boolean): void
}
```

### bootstrap 流程

```
1. 从 settingsStore 读 autoDownload
2. check() → 拿到 Update 句柄（或 null）
3. 若 null/同版本 → idle
4. invoke('updater_check_cache', { version, expected_size })
   ├─ 'complete' → 读字节 → phase: 'ready'
   ├─ 'partial' → 自动模式自动续传，手动模式 phase: 'available'
   └─ 'none' → 自动模式自动下载，手动模式 phase: 'available'
```

### startDownload 流程

```
1. 订阅 'updater:download-progress' 事件
2. 订阅 'updater:download-failed' 事件
3. set phase: 'downloading'
4. invoke('updater_download', { url, version, expected_size, etag })
5. 下载完成 → invoke('updater_read_cached_bytes', { version }) → _cachedBytes
6. set phase: 'ready'
```

### installNow

调 `_update.install(_cachedBytes)` → Tauri 自动验签 → `relaunch()`

## UI 改动

### 设置页（SettingsModal AboutPanel）

新增开关：

```
┌─────────────────────────────────┐
│ 自动下载更新       [✓ 开]       │
│ 关闭后需要手动点击下载            │
│                                 │
│ [检查更新]                       │
└─────────────────────────────────┘
```

### 弹窗 UpdaterPanel

新增 `available` phase UI（手动模式专用）：

```
┌─────────────────────────────────┐
│  新版本可用 v0.5.30        [X]  │
├─────────────────────────────────┤
│  当前 0.5.29 → 新版本 0.5.30   │
│  本次更新内容：                  │
│  · 修复了 xxx                   │
│  [稍后再说]     [立即下载]       │
└─────────────────────────────────┘
```

`failed` phase 新增重试次数显示：

```
┌─────────────────────────────────┐
│  更新失败                  [X]  │
├─────────────────────────────────┤
│  ✗ 已重试 3 次仍失败            │
│  错误：network timeout          │
│            [重试]               │
└───────────────────────────���─────┘
```

### 右上角 UpdateAvailableLink

`available` phase 新增显示（沿用现有多 phase 逻辑）：

| Phase | 显示 |
|---|---|
| `available` | 🔴 "v0.5.30 可用" |
| `downloading` | 🔵 "v0.5.30 下载中 45%" |
| `ready` | 🟢 "v0.5.30 可安装" |
| `failed` | 🟡 "更新失败，点击重试" |

## 设置项持久化

`autoDownload: boolean` 加入 `src/types/settings.ts` 的 `Settings` 接口（默认 `true`），Rust 端对应字段加入 `Settings` struct。通过现有的 `getSettings()`/`updateSettings()` 走 Tauri command 持久化——与现有任何 settings 字段处理方式完全一致。

## 错误处理 & 边界

| 场景 | 处理 |
|---|---|
| 服务器变更（同版本号但 ETag 不同） | 删本地缓存 + 重新下载 |
| 部分下载文件大小对不上 | 删除 .part + 重新开始 |
| install() 验签失败 | 删本地缓存 + phase: 'failed'（不重试） |
| 用户在 downloading 时关闭 app | meta.json 已落盘，下次启动可续 |
| 用户网络中断 | 自动��试 3 次，失败后 failed，用户手动点重试 |
| 磁盘空间不足 | Rust 端写文件 IO error → phase: 'failed' + 错误信息 |
| 验签失败 vs 网络失败 | 分别提示不同文案，签名失败可能是攻击不重试 |

## 测试

- **Rust 单测**：`downloader.rs` 用 mock HTTP server 测续传逻辑、重试逻辑、ETag 变化处理
- **Rust 集成测**：`cache.rs` 用 temp dir 测 meta.json 读写和版本切换清理
- **前端单测**：`updaterStore.test.ts` 适配新状态机，mock Tauri command 和 emit 事件
- **人工测试**：本地打 0.5.28 包，测试 4 个场景
  1. 自动模式完整下载
  2. 自动模式中断后恢复
  3. 手动模式 release notes 流程
  4. 完整缓存跨启动跳过下载

## 不变的部分

- Ed25519 签名验证由 Tauri `Update::install()` 自动做
- `update.json` endpoint 和 OSS 上传流程
- 打包脚本
- 设置/About 页其他部分

## 改动文件清单

### 新增

- `src-tauri/src/updater/mod.rs`
- `src-tauri/src/updater/cache.rs`
- `src-tauri/src/updater/downloader.rs`
- `src-tauri/src/updater/commands.rs`

### 修改

- `src-tauri/src/storage/aijia_home.rs` — 新增 `global_updater_dir()`
- `src-tauri/src/lib.rs` — 注册 5 个新 commands
- `src-tauri/src/<settings module>` — Settings struct 加 `auto_download` 字段
- `src/lib/updaterStore.ts` — 加 `available` phase、`autoDownload` state、`startDownload`/`retryDownload` 方法、订阅 Rust 事件
- `src/components/common/UpdaterPanel.tsx` — 新增 `available` phase UI
- `src/components/layout/UpdateAvailableLink.tsx` — 新增 `available` 显示
- `src/components/settings/SettingsModal.tsx` — 加自动下载开关
- `src/types/settings.ts` — 加 `autoDownload: boolean`
- `src/i18n/zh-CN.json` + `en-US.json` — 新增翻译 key（自动下载开关说明、available 状态等）
- `src/lib/updaterStore.test.ts` — 适配新状态机
