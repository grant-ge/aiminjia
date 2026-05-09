//! Agenda RuntimeTool implementations.
//!
//! 6 tools (spec §7), all binding `organizer = current_persona_id` at
//! construction time via `AgendaToolDeps`:
//!   - create_agenda_item
//!   - list_agenda_items
//!   - update_agenda_item
//!   - cancel_agenda_item     (soft-delete; spec §1.8 / PR-3 patch C)
//!   - skip_occurrence
//!   - list_agenda_occurrences

pub mod cancel;
pub mod create;
pub mod deps;
pub mod list;
pub mod list_occurrences;
pub mod skip;
pub mod update;

pub use cancel::CancelAgendaItemRuntimeTool;
pub use create::CreateAgendaItemRuntimeTool;
pub use deps::AgendaToolDeps;
pub use list::ListAgendaItemsRuntimeTool;
pub use list_occurrences::ListAgendaOccurrencesRuntimeTool;
pub use skip::SkipOccurrenceRuntimeTool;
pub use update::UpdateAgendaItemRuntimeTool;
