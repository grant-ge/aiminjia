use app_lib::runtime::store::{
    AgentInvocationStore, InMemoryAgentInvocationStore, InMemoryRunStore, InMemoryTaskStore,
    InMemoryToolCallStore, RunStore, RuntimeStores, TaskStore, ToolCallStore,
};

#[test]
fn exposes_minimal_runtime_store_contracts() {
    fn assert_runtime_store_bundle<
        R: RunStore,
        T: TaskStore,
        C: ToolCallStore,
        A: AgentInvocationStore,
    >() {
    }

    let _ = RuntimeStores::builder();
    assert_runtime_store_bundle::<
        InMemoryRunStore,
        InMemoryTaskStore,
        InMemoryToolCallStore,
        InMemoryAgentInvocationStore,
    >();
}
