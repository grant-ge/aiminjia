# UI / 平台 — 设计决策归档

> 从 CLAUDE.md 迁出的已稳定设计决策。日常编码参考 CLAUDE.md 中的摘要即可，需要细节时查本文件。

## 租户运行时换肤（C 路径，2026-05-09 重做）

tenant 4 色（`accentColor` / `primaryColor` / `bgColor` / `sidebarBgColor`）+ `productName` + `logoUrl` + `fontFamily` 由 lotus 后台（`tenant-portal/web/src/pages/settings/branding.tsx`，10 个 PRESET_THEMES）下发；后端 `auth/state.rs::TenantInfo` 透传，前端 `src/stores/brandingStore.ts::applyBranding` 派生 ~40 个 CSS 变量（design.pen + legacy `--color-*` 双套命名空间）。派生算法在 `src/lib/themeUtils.ts`：accent 用 lighten/darken/rgba 派生 hover/subtle/-50/-100/...；primary 决定 `--foreground` 和文字色；bg/sidebar 用 `mixColors(bg, fg, ratio)` 派生 muted/border/popover。`isDarkColor` 决定 `--*-foreground` 用 `#FFFFFF` 还是 `#1A1A1A`，所以 dark mode 不需要 `.dark` selector，直接 4 色填深色即可。`authStore` 在 `restoreFromStorage / login` 后调 `applyTenantBranding(info)`，`logout` 时 `useBrandingStore.reset()`。**前端不再提供本地 accent 选择器**（之前 `GeneralPanel` 的强调色 swatch 已删除）——皮肤完全由 lotus 后台定，避免"用户本地选择 vs 租户下发"的覆盖冲突。`localStorage` 里历史残留的 `aijia-accent-color` 和后端 settings 里的 `accentColor` 字段都不再被读取。

## 标题栏 = accent 色（2026-05-09）

mac Overlay 区 + Windows 自绘标题栏统一用 `bg-primary text-primary-foreground` 显示租户 accent，是用户感知最强的换肤点。`App.tsx` 顶层 wrapper 用 `bg-background`（不再是 `bg-sidebar`），让租户 bgColor 一改 wrapper 就跟着变。Windows 标题栏拖拽踩坑：① `flex-1` 占位 div 必须有 `data-tauri-drag-region`，吃掉中间空白区域的拖拽；② `WindowControls` 外层 `onMouseDown stopPropagation`，否则点击关闭按钮会被父级拖拽吞；③ `UpdateAvailableLink` 也必须包 `stopPropagation` 容器，否则点击更新链接没反应（0.3.x 历史 bug）。mac Overlay 拖拽由系统提供，**不要**额外绑 `onMouseDown=startDragging` 否则跟系统冲突。Windows 标题栏底部用 `border-b border-primary-foreground/15` 做半透明分隔线，避免 accent 与 background 色差小时糊在一起。

## 跨平台拖拽上传（v0.5.9）

Tauri 2 webview 拦截 HTML5 drop 事件，React `onDrop` 永不触发；唯一靠谱的入口是 `getCurrentWebview().onDragDropEvent`。`useDragDropListener`（在 `App` 顶层 mount 一次）订阅 native 事件，把 resolved `PendingAttachment[]` push 进 `useDropInbox`（zustand pull queue），HomeTaskComposerCard / ChatBottomArea 各自 useEffect drain。新增附件路径校验（`useChatAttachments::isAcceptablePastedPath` / `makePendingAttachment` / `resolvePastedPaths`）必须支持 `[\\/]` 双分隔符 + Windows `C:\` 卷根 + 系统目录前缀拒绝。

## 剪贴板图片粘贴

`useComposerPaste.handlePaste` 支持截图/复制图片粘贴。流程：同步提取 `clipboardData.items` 中的 image blob（异步后 clipboardData 会被浏览器清空）→ 先尝试 native file paths（Finder 复制文件）→ 路径为空时 fallback 到 image blob → `saveClipboardImage()` 写入 `tmpImage/` → 添加为 `PendingAttachment`。`saveClipboardImage` 从 `useChatAttachments` 传入（Tauri IPC → Rust `save_clipboard_image_to_tmp`）。ChatBottomArea + HomeTaskComposerCard 两个 composer 均已接入。

## 旧 macOS / WebKit 解析期白屏防护（v0.5.29）

Tauri webview = 系统 WebKit（mac）/ WebView2（win）。macOS 12 Monterey 自带 Safari 15.x，bundle 里若含它不支持的语法，WKWebView 在 **parse 阶段**就抛 `SyntaxError`，整个 React tree 崩 → 纯白屏（无 error boundary 兜底）。两类元凶分清楚：① **正则 lookbehind `(?<=` / `(?<!`**（Safari 16.4 才支持）—— `build.target` **无法转译正则**，只能锁依赖版本（已知 `mdast-util-gfm-autolink-literal` 2.0.1 的邮箱 autolink 踩雷，`package.json` `pnpm.overrides` 锁 2.0.0）；② **JS 语法超标** —— Vite 7 默认 target `baseline-widely-available`（≈ Safari 16）本身就不兼容 Monterey，已在 `vite.config.ts` 加 `build.target`（`windows→chrome105 / 其它→safari13`）兜底。注意 target 只降语法，**不 polyfill 运行时 API**（structuredClone / crypto.randomUUID 等）。排查：`grep -roE '\(\?<[=!]' dist/assets/*.js` 应为 0；`highlight.js` 里 r/scala/haskell/gcode 的 lookbehind 全在注释里、压缩剥离、非风险；Node 工具链（vite/vitest/tailwind/eslint）的 lookbehind 不进 webview。

## 窗口标题 = productName（Dock 菜单，v0.5.29）

原生 window title 之前被设成单空格 `" "`，导致 macOS Dock 右键菜单 / Mission Control / Cmd+Tab 的窗口名显示空白。改为：`lib.rs` 启动 `set_title("AIjia")` 作 fallback，`brandingStore.setWindowTitle` 调 `getCurrentWebviewWindow().setTitle(title)` 设为租户 `productName`。`titleBarStyle: Overlay` 不在窗口内渲染标题文字，所以**无视觉副作用**，纯修 OS 层窗口名。

## 登录/注册页 + 全局弹窗关闭按钮（v0.5.29）

未登录态 `AuthGate` 渲染 `LoginPage` 替代主 `AppShell`，因此 `<TitleBar />` 不挂载；Windows 又 `set_decorations(false)`，导致登录/注册页（含内嵌切换的 `RegisterCard`）无任何关窗/最小化按钮。修法：`LoginPage` 顶部内嵌 `<TitleBar />`（mac 走原生红绿灯 overlay，Windows 出自绘 `WindowControls`）。同时 `ui/dialog` 的 `DialogContent` 加内置右上角 X（见「UI 编写规范」），技能市场 / 协议文档等之前只能 Esc/点遮罩的弹窗统一有可见关闭。

## Windows 兼容性约定（v0.5.7）

所有 git 子进程必须传 `-c core.quotepath=false`（中文文件名展示）；用户可编辑 JSON 文件（mcp_config / global_config）的读路径走 `storage::text_io::read_to_string_strip_bom`（剥 Win10 Notepad BOM）；外部 CLI 输出（dws / where.exe / tasklist）解码走 `storage::console_decode::decode_console_bytes`（Windows GBK fallback，靠 `encoding_rs`）；MCP 子进程 spawn 时强制 `PYTHONIOENCODING=utf-8` / `PYTHONUTF8=1` / `LANG=en_US.UTF-8`；hooks runner + skill `!cmd` 替换在 Windows 走 `powershell.exe -NoProfile -Command`（不能裸 `sh -c`）；用户/LLM 提供的文件名走 `storage::safe_filename::ensure_safe_filename`（CON/PRN/COM*/LPT* 保留名 + 禁字符 `<>:"\|?*` + 尾部 `.`/空格 + 长度 ≤ 200）；任何写到磁盘的状态文件优先 tmp + rename 原子写（参考 `runtime::employee::store::write_atomic`），目录删除走 `remove_dir_all_retry` 3×150–300ms backoff。

## Windows 子进程黑窗抑制（v0.5.8）

所有 `Command::spawn` / `.output()` 必须先调 `.no_window()`（`storage::process_ext::NoWindowExt` trait extension），它在 Windows 上注入 `CREATE_NO_WINDOW = 0x08000000` 创建标志，其它平台是 no-op。漏一个就在 Windows 上看到 cmd.exe / conhost.exe 一闪而过。
