# Intel macOS: CSP 太严 + DMG 体积异常

**Date**: 2026-05-19
**Affected versions**: ≤ 0.5.26-beta.3
**Symptom**:
1. 老 Intel Mac 用户安装后样式全丢,console: `type error not allowed by content security policy`
2. v0.5.26-beta.3 Intel DMG 175MB,正常应为 ~97MB(多了 78MB)

## 根因

### 问题 1:CSP 漏配资源类型

`src-tauri/tauri.conf.json` 生产 CSP 当前:

```
default-src 'self'; connect-src 'self' https://*; style-src 'self' 'unsafe-inline';
script-src 'self'; img-src 'self' data: asset: https:
```

缺失:
- `font-src` 未定义 → fallback 到 `default-src 'self'`,**data: URI 字体被挡**(@fontsource 部分子集是 data URI)
- `worker-src` 未定义 → **blob: worker 被挡**(某些依赖懒加载 worker)
- `img-src` 缺 `blob:` → 剪贴板粘贴图片显示失败
- `connect-src` 缺 `ipc: http://ipc.localhost` → Tauri 2 IPC 在 macOS/Linux 走这个 host

新 webview(Apple Silicon、新 macOS 上的 WebKit)对未定义指令较宽容,但**老 Intel Mac 上的 WebKit 严格遵循 spec**:未定义 directive 必须 fallback 到 `default-src`,导致全部资源加载被拒。

### 问题 2:tauri build 不清理 .app target 目录

`build-and-sign-macos.sh` 的 `build_one_arch` 函数已经在 build 之前 stash 掉对方架构的 `src-tauri/resources/runtime/<plat>` 目录,意图让 tauri bundle 只打入本架构 runtime。但 **tauri bundle 对 .app target 目录是 OVERWRITE,不 DELETE**——

```
src-tauri/target/x86_64-apple-darwin/release/bundle/macos/AIjia.app/Contents/Resources/runtime/
```

如果上一次 build 在这里留了 `darwin-arm64/`(stash 修复之前的版本就会),本次 build 即便 source 里只有 `darwin-x64/`,这个 stale 目录也不会被删除 → 进入 codesign → 进入 DMG → 多 78MB。

第一次添加 stash 的 build 不会暴露,但后续 build 永远继承上一次的污染。本仓库 stash 是 3 周前加入的,但用户 target/ 目录里的 .app 是之前(还没 stash)build 留下的。

## 修复

### CSP

放宽 `connect-src` / `style-src` / `script-src` / `img-src` / `font-src` / `worker-src` / `media-src`,与 devCsp 对齐(产线该有的协议头都列上),不放宽 default-src,不引入 unsafe-eval(除 wasm)。

### .app target 清理

`build_one_arch` 在 stash 之后、`tauri build` 之前,删除 `target/<triple>/release/bundle/macos/AIjia.app/Contents/Resources/runtime/`。tauri bundle 会重新创建该目录,只包含 source 里(stash 之后只剩当前架构的)runtime。

## 验证

发布 0.5.26-beta.4 后:
1. Intel DMG 大小应 ≤ 100MB(对比 0.5.26-beta.2 是 97MB)
2. 老 Intel Mac(macOS 11/12 都试)安装后样式正常
3. console 无 CSP 拒绝
