pub struct ReminderBuilder;

impl ReminderBuilder {
    pub fn date_message(today_cn: &str, today_iso: &str) -> serde_json::Value {
        Self::system_reminder_user_message(format!("今天是 {today_cn}（{today_iso}）。"))
    }

    pub fn system_reminder_user_message(content: impl AsRef<str>) -> serde_json::Value {
        serde_json::json!({
            "role": "user",
            "content": format!(
                "<system-reminder>\n{}\n</system-reminder>",
                content.as_ref()
            ),
        })
    }

    pub fn context_message(title: &str, body: &str) -> Option<serde_json::Value> {
        if body.trim().is_empty() {
            return None;
        }
        Some(Self::system_reminder_user_message(format!(
            "# {title}\n\n{body}"
        )))
    }
}
