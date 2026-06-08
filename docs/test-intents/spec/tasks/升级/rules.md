# rules.md — 升级

本 task 测的产品承诺：用户收到桌面端更新后，即使下载、安装、手动检测或自动轮询期间服务端又发布了更新版本，客户端也会重新确认最新更新包，不会继续卡在已经过期的旧版本包上。

UI 文案对应：设置页「检查更新」、更新提示入口、更新面板里的下载进度 / 重试 / 立即安装按钮 / 安装阶段进度。

---

## 测试更新源

使用本地 mock 更新源，不修改线上 `https://lotus.renlijia.com/aijia/update.json`。

启动：

```bash
pnpm dev:updater-intent
```

这个命令会：
- 启动 `http://127.0.0.1:18088/update.json`
- 让 Tauri updater endpoint 指向本地 mock
- 将自动轮询间隔设为 `30000ms`
- 默认提供旧版本 `0.5.34-test.1` 和新版本 `0.5.35-test.1`

切换更新源状态只允许调用控制接口，不能在场景运行中编辑源码、`package.json`、`update.test.json` 或 Tauri 配置文件，避免触发 Vite HMR / 应用重载破坏测试时序。

```bash
curl -sS http://127.0.0.1:18088/control/old-ok
curl -sS http://127.0.0.1:18088/control/old-fail
curl -sS http://127.0.0.1:18088/control/new-ok
curl -sS http://127.0.0.1:18088/control/new-fail
curl -sS http://127.0.0.1:18088/status
```

每条意图开始前先清理下载缓存，避免上一条场景残留：

```bash
rm -rf ~/.renlijia/global/updater
```

---

## 意图-升级-001: 旧包下载失败后，重试下载新包

**场景**
用户看到一个可用更新并开始下载，但这次下载失败了。失败后服务端又发布了更新版本；用户点击重试时，客户端应先刷新最新版本，再下载新包，而不是继续重试已经过期的旧包。

**操作步骤**
1. 清理 `~/.renlijia/global/updater`
2. 设置测试更新源为旧包失败：`curl -sS http://127.0.0.1:18088/control/old-fail`
3. 应用探活：`tauri-pilot aijia health-check`
4. 记录当前时间 `T0`
5. 从设置页点击「检查更新」
6. 等更新面板显示可更新版本 `0.5.34-test.1`
7. 点击更新面板里的下载 / 更新按钮
8. 等更新面板显示下载失败，错误时间在 `T0` 之后
9. 在 30 秒轮询窗口内切到新包正常：`curl -sS http://127.0.0.1:18088/control/new-ok`
10. 点击更新面板里的「重试」按钮
11. 等更新面板显示下载完成 / 可安装状态
12. 查看 `~/.renlijia/logs/renlijia.log` 中 `T0` 之后的 updater 日志

**验收标准**

应该看到：
- 更新面板显示版本 `0.5.35-test.1`
- 更新面板显示下载完成 / 可安装状态
- `~/.renlijia/logs/renlijia.log` 中 `T0` 之后出现 `download_with_resume acquired lock for version=0.5.34-test.1`
- `~/.renlijia/logs/renlijia.log` 中 `T0` 之后出现 `download_with_resume acquired lock for version=0.5.35-test.1`
- `~/.renlijia/global/updater/meta.json` 中 `version == "0.5.35-test.1"`
- `~/.renlijia/global/updater/meta.json` 中 `complete == true`

不应该看到：
- 点击「重试」之后，更新面板仍显示版本 `0.5.34-test.1`
- 点击「重试」之后，`~/.renlijia/logs/renlijia.log` 中继续出现新的 `download_with_resume acquired lock for version=0.5.34-test.1`
- `~/.renlijia/global/updater/meta.json` 中 `version == "0.5.34-test.1"`
- 更新面板出现包含字面值 `Version mismatch` 的错误

---

## 意图-升级-002: 旧包已缓存后，安装前切新包

**场景**
用户已经把一个更新包下载完成，更新面板显示可以安装。此时服务端又发布了更新版本；用户点击安装时，客户端应先刷新最新版本并切到新包下载，不应继续安装旧包后触发版本不一致错误。

**操作步骤**
1. 清理 `~/.renlijia/global/updater`
2. 设置测试更新源为旧包正常：`curl -sS http://127.0.0.1:18088/control/old-ok`
3. 应用探活：`tauri-pilot aijia health-check`
4. 记录当前时间 `T0`
5. 从设置页点击「检查更新」
6. 等更新面板显示可更新版本 `0.5.34-test.1`
7. 点击更新面板里的下载 / 更新按钮
8. 等更新面板显示下载完成 / 可安装状态
9. 在 30 秒轮询窗口内切到新包正常：`curl -sS http://127.0.0.1:18088/control/new-ok`
10. 点击更新面板里的「立即安装」按钮
11. 等更新面板重新显示新版本下载进度，随后显示下载完成 / 可安装状态
12. 查看 `~/.renlijia/logs/renlijia.log` 中 `T0` 之后的 updater 日志

**验收标准**

应该看到：
- 点击「立即安装」后，更新面板显示版本 `0.5.35-test.1`
- 更新面板显示 `0.5.35-test.1` 下载完成 / 可安装状态
- `~/.renlijia/logs/renlijia.log` 中 `T0` 之后出现 `download_with_resume acquired lock for version=0.5.34-test.1`
- `~/.renlijia/logs/renlijia.log` 中 `T0` 之后出现 `download_with_resume acquired lock for version=0.5.35-test.1`
- `~/.renlijia/global/updater/meta.json` 中 `version == "0.5.35-test.1"`
- `~/.renlijia/global/updater/meta.json` 中 `complete == true`

不应该看到：
- `~/.renlijia/logs/renlijia.log` 中 `T0` 之后出现 `updater_install_cached] start, version=0.5.34-test.1`
- 更新面板出现包含字面值 `Version mismatch` 的错误
- `~/.renlijia/global/updater/meta.json` 中 `version == "0.5.34-test.1"`
- 点击「立即安装」后应用立即重启到旧版本

---

## 意图-升级-003: 手动检查时拿到切换后的新包

**场景**
应用已启动，自动轮询还没到下一轮。测试更新源从旧版本切到新版本后，用户主动点击「检查更新」，客户端应拿到新版本。

**操作步骤**
1. 清理 `~/.renlijia/global/updater`
2. 设置测试更新源为旧包正常：`curl -sS http://127.0.0.1:18088/control/old-ok`
3. 应用探活：`tauri-pilot aijia health-check`
4. 记录当前时间 `T0`
5. 在 30 秒轮询窗口内切到新包正常：`curl -sS http://127.0.0.1:18088/control/new-ok`
6. 从设置页点击「检查更新」
7. 等更新面板显示可更新版本 `0.5.35-test.1`

**验收标准**

应该看到：
- 更新面板显示版本 `0.5.35-test.1`
- 更新面板处于可下载状态

不应该看到：
- 更新面板显示版本 `0.5.34-test.1`
- 更新面板出现包含字面值 `Version mismatch` 的错误

---

## 意图-升级-004: 自动轮询后拿到切换后的新包

**场景**
应用已启动并已见过旧版本。测试更新源在轮询间隔内切到新版本后，用户不手动操作，客户端下一轮自动轮询应拿到新版本并进入更新流程。

**操作步骤**
1. 清理 `~/.renlijia/global/updater`
2. 设置测试更新源为旧包正常：`curl -sS http://127.0.0.1:18088/control/old-ok`
3. 应用探活：`tauri-pilot aijia health-check`
4. 记录当前时间 `T0`
5. 等应用首次自动检测到旧版本，或从设置页点击「检查更新」后关闭更新面板
6. 在 30 秒轮询窗口内切到新包正常：`curl -sS http://127.0.0.1:18088/control/new-ok`
7. 不点击「检查更新」和「立即安装」，等待超过一轮轮询时间
8. 打开更新提示入口或更新面板
9. 查看 `~/.renlijia/logs/renlijia.log` 中 `T0` 之后的 updater 日志

**验收标准**

应该看到：
- 下一轮轮询后，更新面板显示版本 `0.5.35-test.1`
- 自动下载完成时，`~/.renlijia/global/updater/meta.json` 中 `version == "0.5.35-test.1"`
- `~/.renlijia/logs/renlijia.log` 中 `T0` 之后出现 `download_with_resume acquired lock for version=0.5.35-test.1`

不应该看到：
- 下一轮轮询后，更新面板仍显示版本 `0.5.34-test.1`
- `~/.renlijia/global/updater/meta.json` 中 `version == "0.5.34-test.1"`
- 更新面板出现包含字面值 `Version mismatch` 的错误

---

## 意图-升级-005: 次新版已就绪，最终安装最新版

**场景**
用户已经把次新版更新包下载完成，更新面板显示可以安装。此时测试更新源切到最新版；用户第一次点击安装时，客户端应先切到最新版并重新下载。最新版下载完成后，用户第二次点击安装，实际安装的也必须是最新版。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 清理 `~/.renlijia/global/updater`
3. 设置测试更新源为旧包正常：`curl -sS http://127.0.0.1:18088/control/old-ok`
4. 记录当前时间 `T0`
5. 从设置页点击「检查更新」
6. 等更新面板显示可更新版本 `0.5.34-test.1`
7. 点击更新面板里的下载 / 更新按钮
8. 等更新面板显示 `0.5.34-test.1` 下载完成 / 可安装状态
9. 在 30 秒轮询窗口内切到新包正常：`curl -sS http://127.0.0.1:18088/control/new-ok`
10. 第一次点击更新面板里的「立即安装」按钮
11. 等更新面板切到 `0.5.35-test.1` 并重新显示下载进度
12. 等更新面板显示 `0.5.35-test.1` 下载完成 / 可安装状态
13. 第二次点击更新面板里的「立即安装」按钮
14. 查看 `~/.renlijia/logs/renlijia.log` 中 `T0` 之后的 updater 日志

**验收标准**

应该看到：
- 第一次点击「立即安装」前，更新面板显示版本 `0.5.34-test.1`
- 第一次点击「立即安装」后，更新面板显示版本 `0.5.35-test.1`
- 第二次点击「立即安装」后，更新面板显示正在安装 `0.5.35-test.1`
- `~/.renlijia/global/updater/meta.json` 中 `version == "0.5.35-test.1"`
- `~/.renlijia/global/updater/meta.json` 中 `complete == true`
- `~/.renlijia/logs/renlijia.log` 中 `T0` 之后出现 `download_with_resume acquired lock for version=0.5.34-test.1`
- `~/.renlijia/logs/renlijia.log` 中 `T0` 之后出现 `download_with_resume acquired lock for version=0.5.35-test.1`
- `~/.renlijia/logs/renlijia.log` 中 `T0` 之后出现 `updater_install_cached] start, version=0.5.35-test.1`
- `~/.renlijia/logs/renlijia.log` 中 `T0` 之后出现 `updater_install_cached] update.version=0.5.35-test.1`

不应该看到：
- 第一次点击「立即安装」后，应用直接安装或重启到 `0.5.34-test.1`
- `~/.renlijia/logs/renlijia.log` 中 `T0` 之后出现 `updater_install_cached] start, version=0.5.34-test.1`
- `~/.renlijia/logs/renlijia.log` 中 `T0` 之后出现 `updater_install_cached] update.version=0.5.34-test.1`
- 更新面板出现包含字面值 `Version mismatch` 的错误
- 第二次点击「立即安装」后，更新面板仍停留在 `0.5.34-test.1`

---

## 意图-升级-006: 安装进行中，显示阶段进度

**场景**
用户已经下载完成一个可安装更新包。用户点击安装后，安装可能在部分设备上耗时较长；更新面板应显示安装阶段进度，而不是长时间停在无反馈状态。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 清理 `~/.renlijia/global/updater`
3. 设置测试更新源为旧包正常：`curl -sS http://127.0.0.1:18088/control/old-ok`
4. 记录当前时间 `T0`
5. 从设置页点击「检查更新」
6. 等更新面板显示可更新版本 `0.5.34-test.1`
7. 点击更新面板里的下载 / 更新按钮
8. 等更新面板显示下载完成 / 可安装状态
9. 点击更新面板里的「立即安装」按钮
10. 在安装完成或开发模式重启失败前，连续观察更新面板的安装文案和进度条
11. 查看 `~/.renlijia/logs/renlijia.log` 中 `T0` 之后的 updater 日志

**验收标准**

应该看到：
- 点击「立即安装」后，更新面板显示正在安装 `0.5.34-test.1`
- 更新面板出现安装阶段进度条
- 更新面板出现安装阶段文案 `正在准备安装包...`
- 更新面板出现安装阶段文案 `正在校验更新版本...`
- 更新面板出现安装阶段文案 `正在安装更新...`
- `~/.renlijia/logs/renlijia.log` 中 `T0` 之后出现 `updater_install_cached] start, version=0.5.34-test.1`
- `~/.renlijia/logs/renlijia.log` 中 `T0` 之后出现 `updater_install_cached] update.version=0.5.34-test.1`

不应该看到：
- 点击「立即安装」后，更新面板长时间没有任何安装阶段文案
- 点击「立即安装」后，更新面板只显示下载进度，不显示安装阶段进度
- 更新面板出现包含字面值 `Version mismatch` 的错误
