# `ChannelConnectionState::NeedsReauth` 变体新增 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给 `ChannelConnectionState` 加 `NeedsReauth` 变体（dingtalk device_code 过期 / whatsapp AuthRevoked / wechat session expired 三家共享同款形状），前后端类型同步，加单测锁层。

**Architecture:** 单一 enum 字段扩展 + TypeScript mirror type 同步 + manager 现有调用点零修改（只是允许新变体存在，没有任何分支强制处理它）。**显式定位为 Phase 3 PR1.5 trait 改造的一部分**——本 plan 跟 telegram phase3 plan §PR1.5 (`docs/superpowers/plans/2026-05-19-im-telegram-phase3.md`) 共享同一个 commit，**不**单独走 PR。

**Tech Stack:** Rust enum + serde camelCase 序列化、TypeScript string literal union、Rust 单测。

**Prerequisites:** 无（这是 Phase 4 / Phase 5 反向需求项目，但本身零外部依赖）。telegram phase3 plan §PR1.5 实施时合并本 plan 的所有 step 到 PR1.5 一次性 commit。

**Spec source:**
- `docs/superpowers/specs/2026-05-18-im-whatsapp-phase4-design.md` §8.3
- `docs/superpowers/specs/2026-05-18-im-wechat-phase5-design.md` L147
- `docs/superpowers/specs/2026-05-18-im-connector-roadmap.md` L51 共享抽象表第 10 行附注

**为什么独立成 plan：** 三个 spec 都引用了"`ChannelConnectionState::NeedsReauth`"这个变体名，但 telegram phase3 plan 在 spec 改路线前写的，没包含它。本 plan 是补丁式追加，实施时挂在 telegram PR1.5 同 commit 里，不污染 telegram plan 文本。

---

## File Structure

```
src-tauri/src/connector/im/types.rs       ← 改：ChannelConnectionState 加 NeedsReauth 变体
src-tauri/src/connector/im/types.rs       ← 加：本变体的 serde 序列化单测
src/lib/tauri.ts                           ← 改：ChannelConnectionState union 加 'needsReauth'
```

3 处改动，全在已存在文件里。**不**创建新文件。

---

## Task 1: Rust 端加 `NeedsReauth` 变体

**Files:**
- Modify: `src-tauri/src/connector/im/types.rs:45-54`
- Test: `src-tauri/src/connector/im/types.rs`（同一文件 `#[cfg(test)] mod tests`）

- [ ] **Step 1: 编辑 enum 定义**

打开 `src-tauri/src/connector/im/types.rs`，找到第 45-54 行 `ChannelConnectionState` enum：

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChannelConnectionState {
    Unconfigured,
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    ConfigError,
}
```

改为（在 `ConfigError` 之后加新变体）：

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChannelConnectionState {
    Unconfigured,
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    ConfigError,
    /// Auth credentials revoked / expired / device unlinked. User must
    /// re-authenticate (re-scan QR for whatsapp / wechat / dingtalk
    /// device_code 过期 等). Distinct from `ConfigError` (用户没配置好) and
    /// `Disconnected` (短暂断开能自动重连).
    NeedsReauth,
}
```

- [ ] **Step 2: 编译**

Run: `cd src-tauri && cargo build --lib`

Expected: 编译通过。manager.rs 的现有 25+ 处 `ChannelConnectionState::xxx` 调用点都构造已有变体，**��**会因为新增 `NeedsReauth` 而失败（Rust 加变体是 non-breaking 操作，除非有 exhaustive match）。

如果出现 "non-exhaustive patterns" 警告 / 错误：grep 找到 `match` block 加一个 `ChannelConnectionState::NeedsReauth => { /* 暂时复用 Disconnected 的处理路径 */ }` 分支。

```bash
cd src-tauri && grep -rn "match.*ChannelConnectionState" src/ tests/ | grep -v "matches!"
```

- [ ] **Step 3: 加序列化单测**

`src-tauri/src/connector/im/types.rs` 末尾如果还没有 `#[cfg(test)] mod tests`，加一个；如果有，在 `mod tests {` 块内追加：

```rust
#[test]
fn needs_reauth_serializes_to_camel_case() {
    let s = serde_json::to_string(&ChannelConnectionState::NeedsReauth).unwrap();
    assert_eq!(s, "\"needsReauth\"");
}

#[test]
fn needs_reauth_deserializes_from_camel_case() {
    let v: ChannelConnectionState = serde_json::from_str("\"needsReauth\"").unwrap();
    assert_eq!(v, ChannelConnectionState::NeedsReauth);
}
```

如果文件原本没有 test mod，连带加 imports：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_reauth_serializes_to_camel_case() {
        let s = serde_json::to_string(&ChannelConnectionState::NeedsReauth).unwrap();
        assert_eq!(s, "\"needsReauth\"");
    }

    #[test]
    fn needs_reauth_deserializes_from_camel_case() {
        let v: ChannelConnectionState = serde_json::from_str("\"needsReauth\"").unwrap();
        assert_eq!(v, ChannelConnectionState::NeedsReauth);
    }
}
```

- [ ] **Step 4: 跑测试**

Run: `cd src-tauri && cargo test --lib connector::im::types::tests`

Expected: PASS (含 2 个新测试 `needs_reauth_serializes_to_camel_case` / `needs_reauth_deserializes_from_camel_case`)

---

## Task 2: 前端 TypeScript mirror type 同步

**Files:**
- Modify: `src/lib/tauri.ts:668-674`

- [ ] **Step 1: 编辑 union type**

打开 `src/lib/tauri.ts`，找到第 668-674 行：

```typescript
export type ChannelConnectionState =
  | 'unconfigured'
  | 'disconnected'
  | 'connecting'
  | 'connected'
  | 'reconnecting'
  | 'configError'
```

改为（在 `'configError'` 之后加新成员）：

```typescript
export type ChannelConnectionState =
  | 'unconfigured'
  | 'disconnected'
  | 'connecting'
  | 'connected'
  | 'reconnecting'
  | 'configError'
  | 'needsReauth'
```

注意：camelCase 跟 Rust 的 `#[serde(rename_all = "camelCase")]` 一致——大小写写错会 runtime 反序列化失败。

- [ ] **Step 2: 前端类型检查**

Run: `pnpm exec tsc --noEmit`

Expected: 通过。如果有 exhaustive switch 处理 `ChannelConnectionState`（前端代码很可能没有 — 大部分前端只是把值往 UI 上塞），TS 会指出来：

```bash
grep -rn "ChannelConnectionState" src/
```

如果某处有 `switch (state)` 没 default → 加 `case 'needsReauth':` 分支，行为暂时跟 `'disconnected'` 一致（PR8 Phase 4 / Phase 5 实施时会真正写 NeedsReauth UI）。

---

## Task 3: Manager 调用点 sanity check

**Files:**
- Read only: `src-tauri/src/connector/im/manager.rs`

- [ ] **Step 1: grep 所有 NeedsReauth 引用点（应只在 types.rs + 单测内）**

```bash
grep -rn "NeedsReauth\|needsReauth" src-tauri/src/ src/
```

Expected: 只有 `src-tauri/src/connector/im/types.rs`（enum 定义 + 单测）+ `src/lib/tauri.ts`（type union）出现 NeedsReauth 命中。

**如果有意外命中点**：说明有人已经在 manager / 别的 connector 偷跑加引用了，需要 sanity check。

- [ ] **Step 2: 验证 manager 现有 ChannelConnectionState match 不出 exhaustive 错误**

Run: `cd src-tauri && cargo build --lib`

Expected: 通过。`manager.rs` 大量调用 `ChannelConnectionState::xxx` 都是构造而非 match 解构，加新变体不会破坏。

- [ ] **Step 3: 跑全 review 测试确认架构层不破**

Run: `cd src-tauri && cargo test review_ --tests --no-fail-fast`

Expected: 已有的 review 测试全绿。

---

## Task 4: 合并到 telegram phase3 PR1.5 commit

**Files:**
- 无新增；本 task 只是 commit 边界约束

- [ ] **Step 1: 验证当前 NeedsReauth 改动跟 telegram PR1.5 改动并存编译通过**

如果你正在按 telegram phase3 plan §PR1.5 走，到 PR1.5 §"Step 2: 编译" 那一步前：把本 plan Task 1+2 都做完。然后一次性跑：

```bash
cd src-tauri && cargo build --lib && cargo test review_ --tests --no-fail-fast
```

Expected: 通过。trait 字段改动 + enum 变体新增独立无关，不会互相干扰。

- [ ] **Step 2: 合并 commit message**

按 telegram phase3 plan §PR1.5 §"Step 4: 一次性提交 PR1.5" 的 commit message 模板基础上追加：

```bash
git commit -m "refactor(connector/im): rename InboundModel→InboundDeployment + add outbound_text_streaming capability + add ChannelConnectionState::NeedsReauth variant (PR1.5)

- rename InboundModel → InboundDeployment (drop NativeDaemon variant)
- add ConnectorCapabilities.outbound_text_streaming: bool (Telegram first true)
- add ChannelConnectionState::NeedsReauth variant (dingtalk device_code expire / whatsapp AuthRevoked / wechat session expire 三家共享形状)
- sweep dingtalk/feishu connector callsites + tests
"
```

注意：commit 一次合并。**不要**把 NeedsReauth 拆分成单独 commit——它跟 PR1.5 共享同样的 "trait 表达力扩展" 范畴。

---

## Self-Review

**1. Spec coverage:**

- ✅ Phase 4 spec §8.3 "ChannelConnectionState 当前可能没有 NeedsReauth 变体" + "Phase 3 PR1.5 trait 改造时统一加" → Task 1
- ✅ Phase 5 spec L147 "Phase 3 PR1.5 改造时统一加（dingtalk device_code 过期 / whatsapp AuthRevoked / wechat session expired 三家共享同款形状）" → Task 1 doc-comment 精准引用
- ✅ Roadmap L51 共享抽象表第 10 行附注 "`ChannelConnectionState::NeedsReauth` 变体一并加入" → 整个 plan
- ✅ 前端 mirror type → Task 2

**2. Placeholder scan:**

- 无 TODO / TBD / "implement later"
- Task 1 Step 2 提到 "如果出现 non-exhaustive patterns 警告" → 不是 placeholder，是分支条件兜底（实际上代码里只用 `matches!` 不会触发，但严谨写出来）
- Task 3 Step 1 "如果有意外命中点" → 同上，sanity check 分支不是 TODO

**3. Type consistency:**

- Rust enum 变体名 `NeedsReauth` 跟 TS type union member `'needsReauth'` 一致（camelCase 转换由 serde 处理）
- 文档注释里 dingtalk / whatsapp / wechat 三个触发场景描述跟 Phase 4 spec §8.1 / Phase 5 spec L147 文案一致

---

## Verification

实施完成后，跑以下验证（不在 task 步骤里，单独跑）：

```bash
# Rust 端
cd src-tauri && cargo test --lib connector::im::types::tests
cd src-tauri && cargo build --lib
cd src-tauri && cargo test review_ --tests --no-fail-fast
cd src-tauri && cargo clippy --tests -- -D warnings

# 前端
pnpm exec tsc --noEmit
pnpm lint
```

所有命令 0 退出 = pass。
