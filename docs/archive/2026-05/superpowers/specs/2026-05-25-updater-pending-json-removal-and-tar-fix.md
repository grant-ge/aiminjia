# Updater: 删除 pending.json 废代码 + 修复 macOS tar 打包

## 背景

自动更新功能自 5/8 上线以来，因 `pending.json` 相关代码反复出问题，累计修了 4 次（e4cc05ee → 9598208b → b55eaf24 → ecd2101c），每次都是 ACL 权限配置不对导致更新主流程被阻断。同时 macOS 打包脚本的 tar 命令未排除 extended attributes，导致 ARM Mac 用户解压更新包失败。

## 问题分析

### pending.json 的设计意图与现实

`pending.json` 的原始设计意图是跨启动记住下载状态：用户关闭 app 后下次启动不用重新下载。但 Tauri 的 `Update` 句柄（下载好的更新包引用）无法跨进程保留，app 重启后句柄丢失，即使读到 `pending.json` 也无法安装，还是得重新下载。

因此 bootstrap 的第一行就是 `await clearPending()`（无条件删除上次的笔记），`readPending` 也从未被调用（挂了 `void readPending` 防 lint 报错）。**pending.json 实际已废弃，但写入代码还留在关键路径上。**

### 反复出问题的根因

代码把 `writePending()` 写在了 `update.download()` 之前，且没有 try/catch。一个非关键的元数据写操作失败，直接阻断了整条更新主链路：

```
writePending()    ← 这步如果 ACL 报错
update.download() ← 这步就永远走不到
```

每次修都在调 ACL scope 的"形状"（加 `/*`、加目录本身），但根本问题是废代码不该有能力拖垮核心流程。

### 历史修复时间线

| 日期 | 提交 | 做了什么 | 遗留问题 |
|------|------|---------|---------|
| 5/8 | f980643c | 新增 pending.json 功能 | 没加 fs ACL 权限 |
| 5/14 | ecd2101c | 6 项边缘 case 加固 | ACL 仍未修 |
| 5/18 | e4cc05ee | 客户反馈后加 5 条 fs 权限，scope `$APPCACHE/updater/*` | 漏了目录本身 `$APPCACHE/updater` |
| 5/20 | 9598208b | scope 加上目录本身，删除 mkdir try/catch | 删 try/catch 让 writePending 变成硬依赖 |

### macOS tar 打包问题

`sign-and-upload-macos.sh` 的 tar 命令未设置 `COPYFILE_DISABLE=1`，macOS 的 `tar` 会把 extended attributes 以 AppleDouble 格式（`._` 前缀文件）嵌入 tar 包。Tauri updater 使用的 Rust tar crate 不认这些条目，解压时报错 `failed to unpack ._AIjia.app`。另外包内还混入了 `.DS_Store` 文件。

## 方案

### 方案 A（采用）：直接删除 pending.json 全部代码

既然每次启动都删、写了也没人读，彻底删掉。连 ACL 里那 5 条 fs 权限也一起删——它们的唯一消费者就是 pending.json。

### 方案 B（未采用）：保留但改成 best-effort

`writePending` 加 try/catch 吞错误。代码改动最小，但留着废代码只会继续惹事。

### 未来优化（如果需要跨启动缓存）

不应在前端通过 `@tauri-apps/plugin-fs` 写文件（受 ACL 限制），应在 Rust 后端通过 Tauri command 做，绕过 capability 层。

## 改动清单

### 1. `src/lib/updaterStore.ts`

- 删除 `PendingMeta` 接口
- 删除 `PENDING_DIRNAME`、`PENDING_FILENAME` 常量
- 删除 `pendingPath()`、`readPending()`、`writePending()`、`clearPending()` 四个函数
- 删除 `bootstrap()` 和 `installNow()` 中所有 pending 调用（`clearPending`、`writePending` × 3）
- 删除不再需要的 import：`@tauri-apps/plugin-fs`（exists/readTextFile/writeTextFile/remove/mkdir）、`@tauri-apps/api/path`（appCacheDir/join）

### 2. `src-tauri/capabilities/default.json`

删除 5 条 scoped fs 权限：`fs:allow-write-text-file`、`fs:allow-read-text-file`、`fs:allow-mkdir`、`fs:allow-remove`、`fs:allow-exists`。保留 `fs:default`（只读访问 app 目录）。

### 3. `src/lib/updaterStore.test.ts`

- 删除 fs 相关 mock（existsMock、readTextFileMock、writeTextFileMock、removeMock、mkdirMock、appCacheDirMock、joinMock）
- 删除 `@tauri-apps/plugin-fs` 和 `@tauri-apps/api/path` 的 mock 块
- 更新涉及 pending.json 的测试用例：第一个改为验证 check() 返回 null 时 phase=idle，第二个删除 pending 相关 mock 设置

### 4. `scripts/sign-and-upload-macos.sh`

```bash
# 改前
tar czf "$TAR" -C "$TAR_DIR" "AIjia.app"

# 改后
COPYFILE_DISABLE=1 tar czf "$TAR" -C "$TAR_DIR" --exclude='.DS_Store' "AIjia.app"
```

## 影响范围

- **三端（ARM Mac / Intel Mac / Windows）**：pending.json ACL 问题彻底消除，更新主流程不再有非关键操作阻断风险
- **ARM Mac**：tar 包不再包含 `._` 文件和 `.DS_Store`，解压不会失败
- **行为变化**：无用户可感知的变化。更新流程仍然是"启动 → 后台下载 → 标题栏提示 → 用户点击安装"，只是不再写/读/删一个从未被使用的 `pending.json`
