use std::path::PathBuf;
use std::sync::Mutex;

pub struct AgendaStore {
    pub(crate) root: PathBuf,
    pub(crate) lock: Mutex<()>,
}
