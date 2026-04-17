/// 工具分类——用于区分原子能力、强能力执行器、编排工具和辅助工具。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    /// 单一动作、单一资源域、低副作用。可直接组合使用。
    Primitive,
    /// 强能力单域执行器，有 session 状态或文件副作用，但仍单段执行。
    Power,
    /// 内部调度多个 Primitive / child run / 多阶段动作。不是基础能力。
    Composite,
    /// 计划、进度、记忆等辅助工具。
    Support,
}

impl Default for ToolKind {
    fn default() -> Self {
        ToolKind::Primitive
    }
}

#[derive(Clone, Debug)]
pub struct ToolDefinition {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub capability_scope: Vec<String>,
    pub kind: ToolKind,
    pub default_read_only: bool,
    pub default_destructive: bool,
}

impl ToolDefinition {
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            display_name: id.clone(),
            id,
            description: description.into(),
            capability_scope: Vec::new(),
            kind: ToolKind::default(),
            default_read_only: false,
            default_destructive: false,
        }
    }

    /// 设置工具分类。
    pub fn with_kind(mut self, kind: ToolKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.default_read_only = read_only;
        self
    }

    pub fn with_destructive(mut self, destructive: bool) -> Self {
        self.default_destructive = destructive;
        self
    }

    /// 设置能力域列表（用于权限管线校验）。
    pub fn with_capability_scope(mut self, scopes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.capability_scope = scopes.into_iter().map(Into::into).collect();
        self
    }
}
