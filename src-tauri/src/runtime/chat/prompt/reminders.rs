pub struct ReminderBuilder;

impl ReminderBuilder {
    pub fn date_message(today_cn: &str, today_iso: &str, weekday_cn: &str) -> serde_json::Value {
        Self::system_reminder_user_message(format!(
            "今天是 {today_cn} {weekday_cn}（{today_iso}）。"
        ))
    }

    pub fn weekday_cn(weekday: chrono::Weekday) -> &'static str {
        match weekday {
            chrono::Weekday::Mon => "星期一",
            chrono::Weekday::Tue => "星期二",
            chrono::Weekday::Wed => "星期三",
            chrono::Weekday::Thu => "星期四",
            chrono::Weekday::Fri => "星期五",
            chrono::Weekday::Sat => "星期六",
            chrono::Weekday::Sun => "星期日",
        }
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
        Some(serde_json::json!({
            "role": "user",
            "isMeta": true,
            "content": format!(
                "<system-reminder>\nAs you answer the user's questions, you can use the following context:\n# {title}\n{body}\n\nIMPORTANT: this context may or may not be relevant to your tasks. You should not respond to this context unless it is highly relevant to your task.\n</system-reminder>\n"
            ),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::ReminderBuilder;

    #[test]
    fn date_message_includes_weekday() {
        let message = ReminderBuilder::date_message("2026年06月09日", "2026-06-09", "星期二");
        let content = message["content"].as_str().unwrap();

        assert!(content.contains("今天是 2026年06月09日 星期二（2026-06-09）。"));
    }
}
