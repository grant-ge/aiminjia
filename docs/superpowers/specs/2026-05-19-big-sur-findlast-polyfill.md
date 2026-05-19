# Big Sur Intel: Array.prototype.findLast TypeError

**Date**: 2026-05-19
**Affected**: macOS Big Sur 11.x Intel(Safari 14 WebKit)
**Symptom**:
```
TypeError: n.findLast is not a function.
  dispatchTransaction — index.js:210
  (Tiptap/ProseMirror focus plugin)
```

## 根因

Tauri 用系统 WebView,**不像 Electron 自带 Chromium**。Big Sur Intel 停在 macOS 11,Safari 14 的 JavaScriptCore 不实现:
- `Array.prototype.findLast` / `findLastIndex` (ES2023, Safari 15.4+)
- `Array.prototype.at` / `String.prototype.at` (ES2022, Safari 15.4+)
- `Object.hasOwn` (ES2022, Safari 15.4+)
- `structuredClone` (Safari 15.4+)

Tiptap 的 focus plugin 在 dispatchTransaction 里直接调 `tr.steps.findLast(...)`,旧 Safari 抛 TypeError → 编辑器 crash → 看似"样式问题"实则是 React 抛错破渲染树。

vite 默认 build target 是 ES2020,**只编译语法不补 prototype 方法**。第三方依赖里调到的 ES2022/2023 方法不会被自动 polyfill。

## 修复

`src/legacy-polyfills.ts`:5 个 polyfill,`if (!proto.method)` 单跳过。`main.tsx` 首条 import,保证在所有依赖模块解析前生效。

注:本次的真实修法是补 polyfill,**不是改 vite target**。改 target 只影响语法(箭头函数等),不影响 runtime 方法存在性。

## 三层 fix 累积(都给 Big Sur Intel)

| 版本 | 问题 | 修法 |
|---|---|---|
| beta.4 | CSP 过严:font-src/worker-src/connect-src ipc 未声明 | tauri.conf.json 放宽 |
| beta.4 | Intel DMG 175MB(残留 + 完整 runtime 双份) | .app target prune + runtime trim |
| **beta.5** | **`findLast` 缺失 → 编辑器 TypeError** | **legacy-polyfills.ts** |

beta.5 是 Big Sur 用户能完整使用编辑器的最低版本。
