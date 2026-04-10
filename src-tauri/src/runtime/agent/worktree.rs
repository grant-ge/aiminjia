#[derive(Clone, Debug)]
pub struct WorktreeContext {
    pub root: String,
    pub branch: Option<String>,
}
