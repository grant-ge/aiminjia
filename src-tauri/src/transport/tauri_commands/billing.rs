//! Tauri commands for account usage and personal-tenant billing.
//!
//! Backed by `/v1/billing/*` and `/v1/enterprise/usage-records` on the Lotus
//! gateway. The user's session key is sourced from `AuthManager` (auto-refreshes
//! if expired), so callers don't have to thread it through.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::auth::AuthManager;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ThisMonth {
    pub year_month: String,
    pub request_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub cost: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SignupBonus {
    pub granted: bool,
    pub amount: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BillingSummary {
    pub balance: String,
    pub currency: String,
    pub this_month: ThisMonth,
    pub signup_bonus: SignupBonus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UsageRecord {
    pub id: u64,
    pub created_at: String,
    pub request_type: String,
    pub model_name: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub cost: String,
    pub key_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct UsageRecordSummary {
    pub request_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub cost: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UsageRecordsPage {
    pub page: u32,
    pub size: u32,
    pub total: i64,
    pub records: Vec<UsageRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<UsageRecordSummary>,
}

#[derive(Debug, Clone, Default)]
pub struct BillingUsageQuery {
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub request_type: Option<String>,
    pub model_name: Option<String>,
}

#[tauri::command]
pub async fn billing_summary(auth: State<'_, Arc<AuthManager>>) -> Result<BillingSummary, String> {
    auth.get_billing_summary()
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn billing_usage_records(
    page: u32,
    size: u32,
    start_at: Option<String>,
    end_at: Option<String>,
    request_type: Option<String>,
    model_name: Option<String>,
    auth: State<'_, Arc<AuthManager>>,
) -> Result<UsageRecordsPage, String> {
    auth.get_billing_usage_records(
        page,
        size,
        BillingUsageQuery {
            start_at,
            end_at,
            request_type,
            model_name,
        },
    )
    .await
    .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn enterprise_usage_records(
    page: u32,
    size: u32,
    start_at: Option<String>,
    end_at: Option<String>,
    request_type: Option<String>,
    model_name: Option<String>,
    auth: State<'_, Arc<AuthManager>>,
) -> Result<UsageRecordsPage, String> {
    auth.get_enterprise_usage_records(
        page,
        size,
        BillingUsageQuery {
            start_at,
            end_at,
            request_type,
            model_name,
        },
    )
    .await
    .map_err(|e| format!("{:#}", e))
}
