pub mod active_runs;
pub(crate) mod cron;
pub mod dispatch_prompt;
pub mod inbox;
pub mod inbox_writer;
pub mod knowledge;
pub mod runner;
pub mod store;
pub mod template_store;

pub use active_runs::{ActiveRun, ActiveRunGuard, EmployeeActiveRuns, TriggerKindLabel};
pub use inbox::{InboxEntry, InboxKind, InboxStore};
pub use runner::{spawn_employee_scheduler, EmployeeRunDispatcher, TriggerKind};
pub use store::{
    CreateEmployeeRequest, DueEmployee, EmployeeRecord, EmployeeStore, UpdateEmployeeRequest,
};
pub use template_store::{
    download_snapshot, ensure_cached, ensure_instance_snapshot, fetch_catalog, fetch_manifest,
    merge_catalog, read_cache, read_instance_snapshot, write_cache, RemoteManifest,
    TemplateManifest, TemplateRef, TemplateSnapshot,
};
