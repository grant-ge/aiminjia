pub mod active_runs;
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
    bootstrap_template, bootstrap_templates, ensure_instance_snapshot, read_instance_snapshot,
    TemplateManifest, TemplateRef, TemplateSnapshot,
};
