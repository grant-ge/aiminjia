# Updater Dialog 重设计：用户主导的下载 + 进度弹窗

## 背景

现有更新流程是"后台静默下载 → 标题栏提示可安装"，但这套流程历史上因 pending.json ACL 权限反复出问题 4 次。更根本的是，用户没有明确的"确认要更新"的交互点——下载在后台自动发生，用户看到提示时已经下载完了，不清楚发生了什么。

## 目标

改为用户主导的更新流程：检测到新版本 → 右上角提示 → 用户点击 → 弹窗展示更新内容 → 用户确认后下载（带进度条）→ 下载完成后用户点击安装重启。

## 状态机

```
idle → checking → available → downloading → ready → installing → idle
                      ↓                        ↑
                   (failed) ←──────────────────┘
```

| Phase | 含义 | 触发方式 |
|---|---|---|
| `idle` | 无更新 / 已安装 | 初始状态 / install 完成 |
| `checking` | 正在检查版本 | `bootstrap()` 调用 `check()` |
| `available` | 有新版本，等用户确认 | `check()` 返回非空且版本 > 当前 |
| `downloading` | 用户确认后下载中 | 用户点击"立即更新" → `startDownload()` |
| `ready` | 下载完成，等用户安装 | `download()` resolve |
| `installing` | 安装中 | 用户点击"安装并重启" → `installNow()` |
| `failed` | 下载/安装失败 | `download()` 或 `install()` reject |

### 与旧状态机的区别

- 新增 `checking` 状态（设置页 loading 指示）
- 新增 `available` 状态（旧流程跳过这步直接下载）
- `bootstrap()` 不再自动调用 `download()`
- 新增 `startDownload()` 方法，由用户点击触发

## Store 改动（updaterStore.ts）

### 新增方法

```typescript
startDownload(): Promise<void>
```

用户点击"立即更新"时调用。内部调用 `_update.download()` 并追踪进度。

### bootstrap() 改动

```typescript
async bootstrap() {
  // 去重检查（保持不变）
  set({ phase: 'checking' })
  const update = await check()
  if (!update || update.version === currentVersion) {
    set({ phase: 'idle' })
    return
  }
  // 停在 available，不自动下载
  set({ phase: 'available', version: update.version, notes: update.body, _update: update })
}
```

### installNow() 改动

保持现有逻辑不变：调用 `_update.install()` → `relaunch()`。

## 右上角提示（UpdateAvailableLink）

根据 phase 显示不同内容：

| Phase | 显示 | 点击行为 |
|---|---|---|
| `available` | 🔴 "v0.5.30 可用" | 打开弹窗 |
| `downloading` | 🔵 "v0.5.30 下载中 45%" | 打开弹窗 |
| `ready` | 🟢 "v0.5.30 可安装" | 打开弹窗 |
| `failed` | 🟡 "更新失败，点击重试" | 调用 bootstrap() 重新检查 |

### TitleBar 改动

当前只在 `phase === 'ready'` 时渲染 `<UpdateAvailableLink />`，改为 `available`、`downloading`、`ready`、`failed` 都渲染。

## 弹窗 UI（UpdaterPanel → Dialog）

用 `@/components/ui/dialog` 的 `Dialog` / `DialogContent` 替代原来的自定义面板。弹窗根据 phase 显示不同内容：

### 阶段 1：available — 展示更新内容

```
┌─────────────────────────────────┐
│  新版本可用 v0.5.30        [X]  │
├─────────────────────────────────┤
│  当前 0.5.29 → 新版本 0.5.30   │
│                                 │
│  本次更新内容：                  │
│  · 修复了 xxx                   │
│  · 新增了 yyy                   │
│                                 │
│  [稍后再说]     [立即更新]       │
└─────────────────────────────────┘
```

- "稍后再说"关闭弹窗，右上角仍显示提示
- "立即更新"调用 `startDownload()`，弹窗切换到阶段 2

### 阶段 2：downloading — 下载进度

```
┌─────────────────────────────────┐
│  正在下载更新 v0.5.30      [X]  │
├─────────────────────────────────┤
│  ████████████░░░░░  65%         │
│  已下载 32.5 MB / 50.0 MB      │
│                                 │
└─────────────────────────────────┘
```

- 关闭弹窗：后台继续下载，右上角显示 "v0.5.30 下载中 65%"
- 重新打开弹窗：继续显示当前进度

### 阶段 3：ready — 安装确认

```
┌─────────────────────────────────┐
│  更新下载完成 v0.5.30      [X]  │
├─────────────────────────────────┤
│  ✓ 下载完成 (50.0 MB)          │
│                                 │
│         [立即安装并重启]         │
└─────────────────────────────────┘
```

- 点击"立即安装并重启"→ `installNow()`
- 关闭弹窗：右上角显示 "v0.5.30 可安装"

### 失败状态

```
┌─────────────────────────────────┐
│  更新失败                  [X]  │
├─────────────────────────────────┤
│  ✗ 下载失败：network timeout    │
│                                 │
│            [重试]               │
└─────────────────────────────────┘
```

- "重试"调用 `startDownload()` 重新下载

## 设置页联动（SettingsModal）

`onCheckUpdate` 行为调整：

```typescript
const onCheckUpdate = async () => {
  const store = useUpdaterStore.getState()
  await store.bootstrap()
  const phase = useUpdaterStore.getState().phase
  if (phase === 'idle') {
    await message(t('settings.about.alreadyLatestVersion'), { title: productName, kind: 'info' })
  } else {
    // available / downloading / ready / failed → 打开弹窗
    store.openPanel()
  }
}
```

与现有逻辑基本一致，只是 `bootstrap()` 不再自动下载，所以 `available` 状态也会打开弹窗。

## 改动文件清单

| 文件 | 改动 |
|---|---|
| `src/lib/updaterStore.ts` | 新增 `checking`/`available` phase，`bootstrap()` 拆分，新增 `startDownload()` |
| `src/components/common/UpdaterPanel.tsx` | 重写为 Dialog，三阶段 UI |
| `src/components/layout/UpdateAvailableLink.tsx` | 支持 `available`/`downloading` 状态显示 |
| `src/components/layout/TitleBar.tsx` | 扩大 link 显示条件 |
| `src/components/settings/SettingsModal.tsx` | 微调 onCheckUpdate（可能无需改动） |
| `src/lib/updaterStore.test.ts` | 更新测试用例匹配新状态机 |
| `src/i18n/zh-CN.json` + `en-US.json` | 新增/调整翻译 key |

## 不变的部分

- `installNow()` 的 install + relaunch 逻辑
- Ed25519 签名校验（Tauri plugin 内部处理）
- `update.json` endpoint 和 OSS 上传流程
- 打包脚本（tar 修复已在另一个 PR）
