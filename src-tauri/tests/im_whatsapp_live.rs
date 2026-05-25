//! WhatsApp 真账号 canary 测试骨架。spec v3 §9.2。
//!
//! 默认 #[ignore] —— CI 不跑。开发者本地接真账号时手动：
//!
//!     WHATSAPP_LIVE_TEST=1 cargo test --test im_whatsapp_live -- --ignored
//!
//! 真账号准备：跑过 pnpm tauri:dev 扫码完成（~/.renlijia/users/default/channels/whatsapp/
//! 下 session.db + config.json 都存在）。然后这些测试复用现有 session，发若干消息验证：
//!
//! - 24h 不掉线
//! - 50 条文本收发无风控
//! - AI Card 编辑路径触发 ≥3 次
//! - NeedsReauth 三场景：手机端登出 / multi-device 切换 / 重新扫码
//!
//! PR8 仅落骨架；真实施 PR9+ 或者手动验证后填入。

fn live_enabled() -> bool {
    std::env::var("WHATSAPP_LIVE_TEST")
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn skip_skeleton() {
    eprintln!("PR8 skeleton — manual canary, see test file docstring");
}

#[test]
#[ignore]
fn live_smoke_inbound_one_text() {
    if !live_enabled() {
        eprintln!("skip: set WHATSAPP_LIVE_TEST=1");
        return;
    }
    skip_skeleton();
}

#[test]
#[ignore]
fn live_smoke_outbound_one_text() {
    if !live_enabled() {
        eprintln!("skip: set WHATSAPP_LIVE_TEST=1");
        return;
    }
    skip_skeleton();
}

#[test]
#[ignore]
fn live_50_messages_no_throttle() {
    if !live_enabled() {
        eprintln!("skip: set WHATSAPP_LIVE_TEST=1");
        return;
    }
    skip_skeleton();
}

#[test]
#[ignore]
fn live_aicard_edit_path_3_times() {
    if !live_enabled() {
        eprintln!("skip: set WHATSAPP_LIVE_TEST=1");
        return;
    }
    skip_skeleton();
}

#[test]
#[ignore]
fn live_needs_reauth_logout_from_phone() {
    if !live_enabled() {
        eprintln!("skip: set WHATSAPP_LIVE_TEST=1");
        return;
    }
    skip_skeleton();
}
