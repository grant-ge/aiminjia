use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::item::AgendaItem;
use super::occurrence::TriggerSource;

#[async_trait]
pub trait AgendaRunDispatcher: Send + Sync {
    /// 异步触发一次执行：创建 conversation、切 persona、发 prompt、等 agent 跑完、回写 occurrence。
    /// 返回触发瞬间的 occurrence_id 让 caller 关联（如 run_now 命令需要返回 occurrence）。
    async fn dispatch(
        &self,
        item: AgendaItem,
        planned_fire_at: DateTime<Utc>,
        trigger_source: TriggerSource,
        now: DateTime<Utc>,
    ) -> Result<String>;
}
