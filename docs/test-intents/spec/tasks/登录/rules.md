# rules.md — 认证（Auth）意图测试规格

## 测试范围
覆盖用户从登录到登出的完整身份链路：手机号 / 邮箱登录、session 过期后的自动刷新、多账号切换、登录后租户品牌信息（brand.json）落盘与生效。仅验证身份与会话状态本身，不覆盖业务功能在登录后的具体表现。

## 测试账号约定

- 若当前已有有效登录态、且本意图不要求重新登录，沿用当前账号。
- 若本轮用户提供了账号密码，令 `$TEST_ACCOUNT` / `$TEST_PASSWORD` 等于用户提供的值。
- 若当前未登录、意图必须走登录表单、且用户未提供账号密码，才使用兜底值：`$TEST_ACCOUNT=test@ai`、`$TEST_PASSWORD=testtest`。
- 报告里只记录 `$TEST_ACCOUNT` 和 scope，不重复输出 `$TEST_PASSWORD`。

## 待覆盖的主要场景
- 场景 1：手机号 + 验证码登录成功，token 落盘，UI 切到已登录态
- 场景 2：邮箱 + 密码登录成功，token 落盘，UI 切到已登录态
- 场景 3：登录失败（错误验证码 / 错误密码 / 账号不存在）时不写入 token，UI 给出明确错误提示
- 场景 4：登出后本地 token / 用户信息 / scope 被清理，敏感缓存（如 brand）一并清空，UI 回到登录页
- 场景 5：session 过期触发自动刷新，刷新成功后请求续跑、用户无感知；访问令牌、刷新令牌、会话密钥全部过期时强制回到登录页
- 场景 6：多账号切换时当前 scope 正确切换，前一账号数据不串到新账号
- 场景 7：登录成功后租户 brand.json（productName / logoUrl / 4 色 / fontFamily 等）写入本地并被 brandingStore 应用，CSS 变量按租户生效

---

## 意图 1：用有效账号登录后，active_account.json 落盘，前端跳转到主界面

**场景**
用户打开应用看到登录页，输入正确的用户名和密码点击登录，应用记住当前活跃账号并把界面切到主页（不再停留在登录卡片）。

**前提**
- 应用已启动，当前未登录（登录页可见，输入框为空或仅有 remember 缓存的用户名）
- 网络可达租户后台 (`auth.renlijia.com` / 配置的 tenant API)
- 有一个已知的有效测试账号 `$TEST_ACCOUNT` + 密码 `$TEST_PASSWORD`，账号取值按「测试账号约定」解析
- `~/.renlijia/global/auth/active_account.json` 在测试前不存在（或先手动删除以确保从 0 开始）

**操作**
1. 在登录卡片「账号」输入框输入 `$TEST_ACCOUNT`
2. 在「密码」输入框输入 `$TEST_PASSWORD`
3. 点击「登录」按钮
4. 等待按钮上的 spinner 消失（最长 10 秒）

**验收标准**
- 登录卡片消失，主界面渲染（左侧侧边栏可见，顶部标题栏出现）
- 文件 `~/.renlijia/global/auth/active_account.json` 存在
- 该文件为合法 JSON，包含 `scopeKey`、`tenantId`、`userId` 三个字段，且三个字段值非空
- 目录 `~/.renlijia/users/t_{tenantId}__u_{userId}/` 存在
- 该目录下 `scope.json` 存在，`username` 字段值等于 `$TEST_ACCOUNT` 的用户标识部分（不含 `@tenant` 后缀；例如登录用 `test@ai` 时 `username == "test"`，登录用手机号 `13800000000@pzctest` 时 `username == "13800000000"`）
- 登录页上方原本显示的「错误提示」区域为空（无 `text-destructive` 文本）

---

## 意图 2：用错误密码登录，UI 显示错误提示，停留在登录页

**场景**
用户输入了一个存在的账号但密码不对，应用不能误认为登录成功，也不能写入任何凭据文件，必须在登录页上给出可读的错误提示让用户重试。

**前提**
- 应用已启动，当前未登录
- 网络可达租户后台
- 已知测试账号 `$TEST_ACCOUNT` 存在且密码 **不是** `Pwd-Wrong-XXX`
- `~/.renlijia/global/auth/active_account.json` 在测试前不存在

**操作**
1. 在登录卡片「账号」输入框输入 `$TEST_ACCOUNT`
2. 在「密码」输入框输入 `Pwd-Wrong-XXX`
3. 点击「登录」按钮
4. 等待按钮上的 spinner 消失

**验收标准**
- 登录卡片仍然可见（未跳到主界面），登录按钮文案恢复为「登录」
- 登录卡片中出现红色错误提示文案（`text-destructive`），文案非空，包含「密码」「凭据」「失败」或「认证」之一
- 「密码」输入框被清空（value 为空字符串）
- 「账号」输入框仍然显示 `$TEST_ACCOUNT`（未被清空）
- 文件 `~/.renlijia/global/auth/active_account.json` **不存在**
- 目录 `~/.renlijia/users/` 下没有为本次失败账号新建的 `t_*__u_*` 子目录

---

## 意图 3：用户点击登出后，active_account.json 被清除，前端跳转回登录页

**场景**
已登录用户在设置或头像菜单里点击「退出登录」，应用应立即把本地的活跃账号标记清掉，回到登录页，不再让任何主界面功能可访问。

**前提**
- 应用已启动且当前用户已登录（主界面可见，侧边栏头像显示当前用户名）
- 文件 `~/.renlijia/global/auth/active_account.json` 存在，内容为当前登录用户的 scopeKey / tenantId / userId

**操作**
1. 在主界面打开当前账号下拉菜单（点头像或顶栏账号按钮）
2. 点击「退出登录」菜单项
3. 若出现二次确认弹窗，点击「确认退出」

**验收标准**
- 主界面消失，登录卡片重新出现（`<LoginCard>` 可见，「账号」输入框可被聚焦）
- 文件 `~/.renlijia/global/auth/active_account.json` 不存在（已被删除或者其内容已被清空到不含 `scopeKey` 字段）
- 前端 `useAuthStore` 状态对应的 `isLoggedIn` 在 DevTools / 调试输出中为 `false`
- 登出后立即在登录页输入相同账号 + 正确密码 → 能再次走完意图 1 的验收（即登出未损坏后续登录路径）

---

## 意图 4：登录后凭据落盘，重启应用后自动恢复登录态，无需重新输入账号

**场景**
用户在桌面端登录一次后，关闭应用再重新打开，应用应通过本地凭据自动登录，不让用户再看到登录卡片，也不要求重新输入密码。

**前提**
- 应用已启动并已登录（按意图 1 完成登录）
- `~/.renlijia/global/auth/active_account.json` 存在
- `~/.renlijia/global/auth/` 目录下存在 `cloud_auth` 文件（AES-256-GCM 加密的 token 容器，`auth_dir().join("cloud_auth")`）
- 已知登录账号为 `$TEST_ACCOUNT`

**操作**
1. 通过菜单「文件 → 退出 AIjia」或系统快捷键完全关闭应用进程（不是登出，是退出应用）
2. 重新从 Dock / 启动器打开应用
3. 等待启动闪屏消失（最长 15 秒）

**验收标准**
- 应用重新打开后**直接进入主界面**，不显示登录卡片（无「账号」输入框、无「登录」按钮）
- 顶栏 / 侧边栏头像区域显示 `$TEST_ACCOUNT` 对应的用户名或租户后台展示名
- `~/.renlijia/global/auth/active_account.json` 与 `~/.renlijia/global/auth/cloud_auth` 两个文件均存在，重启前后 sha256 不变（仅 `lastSeenAt` 字段可能变化）
- 前端 `useAuthStore.isLoggedIn` 为 `true`，`user.username` 字段值等于 `$TEST_ACCOUNT` 的用户标识部分或租户后台对应的用户标识
- 整个启动过程未触发对登录页 `cloudLogin` 命令（可用日志 `[cloud_login]` 缺席间接验证；同等条件下重启前的成功登录会打印一行 `[cloud_login] user=...`，重启自动恢复不打印）

---

## 意图 5：登录成功后租户品牌信息落盘，前端主题色与租户配置一致

**场景**
租户在 lotus 后台配置了自定义产品名 / logo / 4 色调色板。用户用属于该租户的账号登录后，应用窗口的主题色、登录页 logo、顶栏 productName 必须立刻切到租户品牌，并把品牌快照写盘以便下次冷启动时登录页能预先呈现。

**前提**
- 应用已启动，当前未登录
- 测试用账号属于一个**已在 lotus 后台配置非默认品牌**的租户（非空 `productName`、`accentColor` 不等于默认值）；记该租户后台配置的 accentColor 为 `$ACCENT_COLOR`、productName 为 `$PRODUCT_NAME`
- `~/.renlijia/users/{scope}/brand.json` 在测试前不存在（首次登录场景）

**操作**
1. 在登录卡片输入有效账号 + 密码
2. 点击「登录」按钮
3. 等待主界面渲染完成（侧边栏出现）

**验收标准**

应该看到：
- 文件 `~/.renlijia/users/t_{tenantId}__u_{userId}/brand.json` 存在
- 该文件为合法 JSON，至少包含 `productName`、`accentColor`、`primaryColor`、`bgColor`、`sidebarBgColor` 五个字段
- `productName` 字段值等于 `$PRODUCT_NAME`；`accentColor` 字段值等于 `$ACCENT_COLOR`（大小写不敏感比较）
- 在 Webview DevTools 中检查 `:root` 元素，`--primary` CSS 变量解析值等于 `$ACCENT_COLOR`（或等价 rgb 值）
- 顶栏节点 `[data-aijia-product-name]` 的可见文本等于 `$PRODUCT_NAME`，不显示默认的 `"AI小家"`
- 登出（按意图 3 操作）后再回到登录页，登录页 logo 区域使用 `brand.json` 中保存的 `logoUrl`（即不闪回默认 logo）

不应该看到：
- `~/.renlijia/` 下完全找不到任何包含 `brand` 的文件（说明 brandingStore 没把品牌信息持久化）
- 顶栏 `[data-aijia-product-name]` 节点缺失（说明 selector 还没加到 LoginCard / 顶栏）
- `brand.json` 字段值与 lotus 后台配置不一致（写盘错位）

---

## 意图-登录-006: 登录态过期，回到登录页

**场景**
用户本地还残留登录凭据，但访问令牌、刷新令牌和会话密钥都已经过期。应用不能继续停留在可聊天的已登录态，也不能把这个身份问题展示成 API Key 配置问题；必须让用户回到登录页重新登录。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 记录当前页面状态：`tauri-pilot aijia where --json`，记下 `T0=$(date -u +%Y-%m-%dT%H:%M:%SZ)`。
3. 若当前未登录，按「测试账号约定」解析 `$TEST_ACCOUNT` / `$TEST_PASSWORD` 后登录：`tauri-pilot aijia login --account "$TEST_ACCOUNT" --password "$TEST_PASSWORD" --json`。
4. 运行 `node scripts/test-intents/expire-cloud-auth.mjs expire`，把 `~/.renlijia/global/auth/cloud_auth` 中的 `accessExpiresAt`、`refreshExpiresAt`、`sessionKeyExpiresAt` 写到过去时间，并把原文件备份到 `~/.renlijia/global/auth/cloud_auth.intent-expire.bak`。
5. 运行 `node scripts/test-intents/expire-cloud-auth.mjs status`，记录输出里的 `expiredAll`、`accessExpiresAt`、`refreshExpiresAt`、`sessionKeyExpiresAt`。
6. 重启应用并等待完成：`tauri-pilot aijia restart-app --json` 后再跑 `tauri-pilot aijia health-check`。
7. 若重启后仍停留在主界面，创建新对话并发送 `e2e-test-auth-expired`：`tauri-pilot aijia new-task`、`tauri-pilot aijia type-message "e2e-test-auth-expired"`、`tauri-pilot aijia send`，等待页面状态变化。
8. 采样最终状态：`tauri-pilot aijia where --json` 和 `tauri-pilot aijia ui-message --include-empty --json`。
9. 验收完成后恢复凭据：`node scripts/test-intents/expire-cloud-auth.mjs restore --delete-backup`。

**验收标准**

应该看到：
- 步骤 5 的 `expiredAll == true`
- 步骤 5 的 `accessExpiresAt == "2000-01-01T00:00:00Z"`
- 步骤 5 的 `refreshExpiresAt == "2000-01-01T00:00:00Z"`
- 步骤 5 的 `sessionKeyExpiresAt == "2000-01-01T00:00:00Z"`
- 最终页面出现登录页的「账号」输入框
- 最终页面出现登录页的「密码」输入框
- 最终页面出现登录页的「登录」按钮
- 页面可见文案或最近消息文本包含「请重新登录」
- `~/.renlijia/global/auth/cloud_auth.intent-expire.bak` 在步骤 9 后不存在

不应该看到：
- 页面可见文案或最近消息文本中出现「API 密钥无效或已过期」
- 页面可见文案或最近消息文本中出现「检查 API Key 配置」
- 页面可见文案或最近消息文本中出现「无法获取会话密钥」
- 步骤 7 发送的 `e2e-test-auth-expired` 产生 assistant 回复正文
- 最终页面仍显示可输入的主界面聊天输入框
