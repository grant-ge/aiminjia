# Phase 4 WhatsApp PR8 — 集成测试 + UI finalize + canary 骨架

> Subagent-driven execution. 4 decisions confirmed by user.

---

## Context

PR1-PR7 后端 + PR3 扫码 UI 都跑通了。PR8 收尾：

- `allow_from` 配置 UI（PR3 已加 banner / config 卡片入口，PR8 加 textarea + Tauri 命令）
- NeedsReauth 卡片态（重扫码 onClick）
- review_im_layering explicit assert（锁 whatsapp 在 platforms）
- 真账号 canary `tests/im_whatsapp_live.rs --ignored` 骨架（spec §9.2，CI 不跑）

不做：mock wa-rs 走 e2e 集成测试（spec §12.2 提的，但 wa-rs Bot 不是 trait，抽象成本太重；canary 替代）。spec §8.4 三场景文案（要重构 last_error 为结构化 JSON，留后续 PR）。

Spec: §9.1（PR3 已实现 banner verbatim，PR8 不动）+ §9.2 实测义务 + §10.2 PR8 + §3.10 allow_from。

## 4 decisions (user-confirmed)

1. **集成测试**：只加 `tests/im_whatsapp_live.rs --ignored` 真账号骨架 + `WHATSAPP_LIVE_TEST=1` 环境变量门控。CI 不跑。
2. **allow_from UI**：在现有 `WhatsappChannelConfig` dialog 增加 textarea + 新 Tauri 命令 `channel_whatsapp_update_allow_from(allow_from: Vec<String>)` 读 config.json → mut → write。
3. **NeedsReauth UI**：ChannelPage `statusMeta` 加 `case 'needsReauth'` → `'会话失效' / error`；卡片 onClick 当 needsReauth 时调 begin_whatsapp_registration 走重扫码（不另设按钮）。
4. **review_im_layering**：加一行 explicit `assert!(platforms.iter().any(|p| p == "whatsapp"))`。

---

## File structure

新建：
- `src-tauri/tests/im_whatsapp_live.rs` — `#[ignore]` 真账号 canary 骨架。骨架就行，
  不真跑 —— 留给开发者本地 `cargo test --test im_whatsapp_live -- --ignored` 跑。
  ~80 行。包含 5 个 `#[ignore]` 测试占位（24h 不掉线 / 50 条收发 / 3 个 NeedsReauth 场景）。

修改：
- `src-tauri/src/commands/channel.rs` — 加 Tauri 命令 `channel_whatsapp_update_allow_from`
- `src-tauri/src/connector/im/manager.rs` — 加 helper `update_whatsapp_allow_from(allow_from)` 读 config.json → mut → write
- `src-tauri/src/lib.rs` — 注册新命令（grep `tauri::generate_handler!`）
- `src-tauri/tests/review_im_layering.rs` — 加 `assert_platforms_contains_whatsapp` 测试
- `src/features/channel/WhatsappChannelConfig.tsx` — 加 allow_from textarea + 调用新命令
- `src/features/channel/ChannelPage.tsx` — `statusMeta` 加 `needsReauth` case + 卡片 onClick 处理
- `src/lib/tauri.ts` — 加 `channelWhatsappUpdateAllowFrom` 类型化封装

不动：connector / parser / runtime / sender / aicard / download / gc / config.rs 的字段（config.rs `WhatsAppChannelConfig.allow_from` 已存在）。

---

## Task 1 — Backend Tauri 命令 + manager helper（更新 allow_from）

修改 `src-tauri/src/connector/im/manager.rs`：加 inherent method（放 `connect_whatsapp_from_store` 附近）：

```rust
/// Phase 4 PR8：更新 allow_from allowlist。读现有 config.json → 写 allow_from → 落盘。
/// 调用方负责把字符串数组里的号码规整成 E.164 形态（前端 sanitize）。
/// allow_from 是 [] 时存 None（删除字段）；非空时存 Some(vec)。
pub async fn update_whatsapp_allow_from(&self, allow_from: Vec<String>) -> Result<()> {
    let paths = self.resolve_whatsapp_paths()?;
    let mut cfg = super::whatsapp::config::read(&paths.config_path())?
        .ok_or_else(|| anyhow::anyhow!("whatsapp not paired yet"))?;
    cfg.allow_from = if allow_from.is_empty() { None } else { Some(allow_from) };
    super::whatsapp::config::write(&paths.config_path(), &cfg)?;
    log::info!(
        "[channel/whatsapp] allow_from updated: {} entries",
        cfg.allow_from.as_ref().map(|v| v.len()).unwrap_or(0)
    );
    Ok(())
}
```

⚠️ grep `resolve_whatsapp_paths` 确认存在；PR3 已加。

修改 `src-tauri/src/commands/channel.rs`：加新命令：

```rust
#[tauri::command]
pub async fn channel_whatsapp_update_allow_from(
    app: tauri::AppHandle,
    allow_from: Vec<String>,
) -> Result<(), String> {
    manager(&app)?
        .update_whatsapp_allow_from(allow_from)
        .await
        .map_err(|e| format!("{:#}", e))
}
```

⚠️ grep 现有命令 shape (`channel_begin_registration` etc.) 来参考 attribute / app handle pattern。

修改 `src-tauri/src/lib.rs`：在 `tauri::generate_handler!` 宏里加 `channel_whatsapp_update_allow_from`（grep `channel_begin_registration` 找到 handler 数组）。

### Verification

```bash
cd src-tauri && cargo build --lib 2>&1 | tail -10
cd src-tauri && cargo test --lib connector::im::whatsapp 2>&1 | tail -10
cd src-tauri && cargo clippy --lib --message-format=short 2>&1 | grep -E 'commands/channel|connector/im/manager' | head -10
cd src-tauri && cargo fmt -- --check 2>&1 | grep -E 'commands/channel|connector/im/manager' || echo OK
```

### Commit

```bash
git add src-tauri/src/connector/im/manager.rs \
        src-tauri/src/commands/channel.rs \
        src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR8 加 Tauri 命令更新 allow_from

spec v3 §3.10。

manager.update_whatsapp_allow_from(allow_from)：
- 读 ~/.renlijia/users/{scope}/channels/whatsapp/config.json
- 设 cfg.allow_from = if empty None else Some(vec)
- 写回 config.json

命令 channel_whatsapp_update_allow_from(allow_from: Vec<String>) 暴露。
config 已 paired 才能改（未 paired 返 \"whatsapp not paired yet\"）。

PR4 runtime::handle_event 每条入站事件 fs::read config.json 拿 allow_from
（zero-cache 设计），所以本命令写完下一条消息就自动按新 allow_from 过滤。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2 — Frontend allow_from UI + NeedsReauth UI

修改 `src/lib/tauri.ts`：加 export

```ts
export async function channelWhatsappUpdateAllowFrom(allowFrom: string[]): Promise<void> {
  await invoke('channel_whatsapp_update_allow_from', { allowFrom })
}
```

⚠️ grep 现有 channel- 函数 shape 来对齐 invoke 参数命名（camelCase / snake_case）。

修改 `src/features/channel/WhatsappChannelConfig.tsx`：

1. 加一个新区域"Allow From / 允许的发送人"，下面是 textarea：
   - 一行一个 E.164 号码（占位 `+8613912345678`）
   - 保存按钮调 `channelWhatsappUpdateAllowFrom(parsed_list)`
   - parse helper：每行 trim、去 `-` `空格`、加 `+` 前缀 if 没有、空行 skip、规整后 E.164 校验（`/^\+[1-9]\d{6,14}$/`）
   - 保存成功 toast `"已更新允许列表（N 项）"`，空数组也允许（清空 → 接收所有）
2. 仅在 already paired 时显示（PR3 dialog 已分 idle / risk_banner / modal / **post_pair** 之类的态；如果没有就用 props 透传 `currentJid` 控制）。简化：whatsapp config dialog 进来就显示 allow_from textarea，未 paired 时 "未连接" 灰显 + 保存按钮 disabled。

修改 `src/features/channel/ChannelPage.tsx`：

1. `statusMeta` switch 加：
   ```tsx
   case 'needsReauth':
     return { statusLabel: '会话失效', statusTone: 'error' as const }
   ```
   原 fallthrough `default → 未配置` 保留给其它 unknown state。
2. 移除 line 83-85 TODO 注释（NeedsReauth 现已显式处理）。
3. **重扫码 onClick**：找卡片 click handler（grep `whatsappRegistrationOpen`），在 platform.key === 'whatsapp' 时同样支持重新打开 dialog。如果当前状态 `needsReauth`，dialog 进入 risk_banner phase（自动）或直接进 modal（视 dialog state 实现）。**最简方案**：current onClick 当 needsReauth 时也照常打开 dialog —— `WhatsappChannelConfig` 进 dialog 后用户按"添加 / 重新连接"按钮走 begin。改 button label 取决于 connection state（"添加" / "重新连接"）。

   ⚠️ 这里取决于 PR3 dialog 现状，implementer 看现有逻辑选最简改法。

### Verification

```bash
pnpm exec tsc --noEmit 2>&1 | tail -5
pnpm lint src/features/channel/WhatsappChannelConfig.tsx src/features/channel/ChannelPage.tsx src/lib/tauri.ts 2>&1 | tail -10 || true
```

### Commit

```bash
git add src/lib/tauri.ts \
        src/features/channel/WhatsappChannelConfig.tsx \
        src/features/channel/ChannelPage.tsx
git commit -m "$(cat <<'EOF'
feat(channel/whatsapp): PR8 allow_from UI + NeedsReauth 显式态

spec v3 §3.10 + §8.4。

WhatsappChannelConfig 加 "允许的发送人" 区域：
- textarea 一行一个 E.164 号码（占位 +8613912345678）
- 保存调 channelWhatsappUpdateAllowFrom(parsed_list)
- parse：trim / 去 - 空格 / 加 + 前缀 / 校验 ^\+[1-9]\d{6,14}$ / 空行 skip
- 空数组允许（清空 → 接收所有）；非空 → 仅这些号码触发 AI 回复

ChannelPage statusMeta 加 needsReauth case → "会话失效" / error tone；
原 TODO 注释删除。卡片 onClick 当 needsReauth 时仍打开 dialog 走重扫码。

tauri.ts 加 channelWhatsappUpdateAllowFrom invoke 类型化封装。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3 — review_im_layering explicit assert + canary 骨架

修改 `src-tauri/tests/review_im_layering.rs`：找 `platforms` array 旁加：

```rust
#[test]
fn whatsapp_is_registered_in_platforms_array() {
    // Phase 4 PR8: lock that future dir-scan changes do not silently exclude
    // whatsapp. Spec §10.2 PR8 deliverable.
    let platforms = list_platform_subdirs();  // 或 grep 现有 helper / inline 用代码扫描
    assert!(
        platforms.iter().any(|p| p == "whatsapp"),
        "whatsapp connector dir must be discovered by review_im_layering scan; found: {:?}",
        platforms,
    );
}
```

⚠️ grep 现有 test 怎么 list dir entries；可能现有的 `platforms` 是 `const &[&str]` 而不是动态 scan。如果是 const，**改成动态 scan** 让此 assert 真有意义；如果不能改，那加 `assert!(platforms.contains(&"whatsapp"))` 锁数组成员即可。

新建 `src-tauri/tests/im_whatsapp_live.rs`：

```rust
//! WhatsApp 真账号 canary 测试骨架。spec v3 §9.2。
//!
//! 默认 `#[ignore]` —— CI 不跑。开发者本地接���账号时手动：
//!
//!     WHATSAPP_LIVE_TEST=1 cargo test --test im_whatsapp_live -- --ignored
//!
//! 真账号准备：跑过 `pnpm tauri:dev` 扫码完成（~/.renlijia/users/default/channels/whatsapp/
//! 下 session.db + config.json 都存在）。然后这些测试复用现有 session，发若干消息验证：
//!
//! - 24h 不掉线
//! - 50 条文本收发无风控
//! - AI Card 编辑路径触发 ≥3 次
//! - NeedsReauth 三场景：手机端登出 / multi-device 切换 / 重新扫码
//!
//! PR8 仅落骨架；真实施 PR9+ 或者手动验证后填入。

#![cfg(test)]

fn live_enabled() -> bool {
    std::env::var("WHATSAPP_LIVE_TEST").map(|v| v == "1").unwrap_or(false)
}

#[test]
#[ignore]
fn live_smoke_inbound_one_text() {
    if !live_enabled() {
        eprintln!("skip: set WHATSAPP_LIVE_TEST=1");
        return;
    }
    // 手动验证步骤：从手机给桌面 AIjia 发一条文本 -> AI 回复，看后端 log。
    // 自动化版本待 PR9+。
    todo!("manual canary; see test docstring");
}

#[test]
#[ignore]
fn live_smoke_outbound_one_text() {
    if !live_enabled() {
        eprintln!("skip: set WHATSAPP_LIVE_TEST=1");
        return;
    }
    todo!("manual canary; see test docstring");
}

#[test]
#[ignore]
fn live_50_messages_no_throttle() {
    if !live_enabled() {
        eprintln!("skip: set WHATSAPP_LIVE_TEST=1");
        return;
    }
    todo!("manual canary; see test docstring");
}

#[test]
#[ignore]
fn live_aicard_edit_path_3_times() {
    if !live_enabled() {
        eprintln!("skip: set WHATSAPP_LIVE_TEST=1");
        return;
    }
    todo!("manual canary; see test docstring");
}

#[test]
#[ignore]
fn live_needs_reauth_logout_from_phone() {
    if !live_enabled() {
        eprintln!("skip: set WHATSAPP_LIVE_TEST=1");
        return;
    }
    todo!("manual canary; see test docstring");
}
```

⚠️ `todo!()` 在 `#[ignore]` 测试里通常允许（测试不跑就不 panic）。但 plan 规则"no unimplemented!/todo!"我们打个例外，因为这是 stub 骨架。**或者**改成 `unimplemented_skeleton_call()` helper return 让 `--ignored` + WHATSAPP_LIVE_TEST=1 时也 silently skip：

```rust
fn skip_skeleton() {
    eprintln!("PR8 PR8 skeleton — manual canary, see docstring");
}
```

然后 test body 调 `skip_skeleton()` 而不是 `todo!()`。

### Verification

```bash
cd src-tauri && cargo test --test review_im_layering 2>&1 | tail -5  # 4 passed (3 + 1 new)
cd src-tauri && cargo test --test im_whatsapp_live 2>&1 | tail -5    # 0 run (all ignored)
cd src-tauri && cargo test --test im_whatsapp_live -- --ignored 2>&1 | tail -10  # 5 ignored skeleton calls
cd src-tauri && cargo build --tests 2>&1 | tail -10
cd src-tauri && cargo clippy --tests --message-format=short 2>&1 | grep -E 'tests/im_whatsapp_live|tests/review_im_layering' | head -5
cd src-tauri && cargo fmt -- --check 2>&1 | grep -E 'tests/im_whatsapp' || echo OK
```

### Commit

```bash
git add src-tauri/tests/im_whatsapp_live.rs src-tauri/tests/review_im_layering.rs
git commit -m "$(cat <<'EOF'
test(connector/im/whatsapp): PR8 加 layering assert + live canary 骨架

spec v3 §9.2 + §10.2。

review_im_layering 加 explicit assert whatsapp 在 platforms 数组（防止
未来改 dir-scan logic 时静默漏掉 whatsapp）。

新增 tests/im_whatsapp_live.rs —— 真账号 canary 骨架，5 个 #[ignore] 测试：
- live_smoke_inbound_one_text
- live_smoke_outbound_one_text
- live_50_messages_no_throttle
- live_aicard_edit_path_3_times
- live_needs_reauth_logout_from_phone

默认 #[ignore]，CI 不跑。本地接真账号跑：
WHATSAPP_LIVE_TEST=1 cargo test --test im_whatsapp_live -- --ignored

实施时各 case 在 docstring 描述手动验证步骤；自动化版本留 PR9+。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4 — 收尾

```bash
cd src-tauri && cargo build --lib 2>&1 | tail -5
cd src-tauri && cargo test --lib connector::im::whatsapp 2>&1 | tail -5    # 92 still
cd src-tauri && cargo test --lib connector::im 2>&1 | grep -E '^test result' | tail -3  # 8 pre-existing failures
cd src-tauri && cargo test --test review_im_layering 2>&1 | tail -3        # 4 passed
cd src-tauri && cargo clippy --lib --message-format=short 2>&1 | grep -E 'whatsapp/|commands/channel|manager.rs' | head -15
cd src-tauri && cargo fmt -- --check 2>&1 | head -5
cd .. && pnpm exec tsc --noEmit 2>&1 | tail -3
cd .. && pnpm lint src/features/channel/WhatsappChannelConfig.tsx src/features/channel/ChannelPage.tsx src/lib/tauri.ts 2>&1 | tail -5 || true
```

更新 memory: PR8 行 ✅ + Phase 4 整体完成标记。

---

## Self-Review

| spec | task |
|---|---|
| §3.10 allow_from UI | Task 1 + Task 2 |
| §9.1 风险 banner | PR3 已实现，PR8 不动 |
| §9.2 真账号 canary | Task 3 live skeleton |
| §10.2 review_im_layering | Task 3 explicit assert |
| §8.4 NeedsReauth UI | Task 2 statusMeta needsReauth case（最小化 v1，不分 3 场景） |

不在 PR8 范围（明确避免）：
- spec §12.2 mock wa-rs 完整 e2e 集成测试（成本太重，canary 替代）
- spec §8.4 三场景文案区分（要 last_error 结构化 JSON，留后续）
- PR3 已实施的 §9.1 banner（不动）
- 任何后端 connector / parser / runtime 代码（已 PR1-PR7 落地）

执行：task 1（后端）独立、task 2（前端）独立 — **并行**派；task 3（测试）独立可并行；task 4 collat。3 个任务都可同时派。
