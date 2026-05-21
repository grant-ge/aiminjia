//! iLink endpoint paths + base URL. Verified against
//! openclaw-weixin-main/src/api/api.ts.

/// Default base URL. After QR confirmation, switch to the `baseurl` from the
/// `get_qrcode_status` response (IDC routing — spec §1).
pub const DEFAULT_BASE_URL: &str = "https://ilinkai.weixin.qq.com";

// POST endpoints (Phase 5 PR4 wires these into runtime.rs).
pub const GET_UPDATES: &str = "ilink/bot/getupdates";
pub const SEND_MESSAGE: &str = "ilink/bot/sendmessage";
pub const GET_UPLOAD_URL: &str = "ilink/bot/getuploadurl";
pub const GET_CONFIG: &str = "ilink/bot/getconfig";
pub const SEND_TYPING: &str = "ilink/bot/sendtyping";

// GET endpoints (login.rs uses these).
pub const GET_BOT_QRCODE: &str = "ilink/bot/get_bot_qrcode";
pub const GET_QRCODE_STATUS: &str = "ilink/bot/get_qrcode_status";

/// `bot_type` query param for `get_bot_qrcode`. Verified value from openclaw.
pub const DEFAULT_BOT_TYPE: &str = "3";
