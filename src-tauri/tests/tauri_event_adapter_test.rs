use app_lib::runtime::events::RuntimeEvent;
use app_lib::runtime::ids::{RunId, SessionId};
use app_lib::transport::tauri_event_adapter::map_runtime_event;

#[test]
fn maps_runtime_stream_delta_to_legacy_streaming_delta() {
    let event =
        RuntimeEvent::stream_delta(SessionId::new("conv-1"), RunId::new("run-1"), "hi".into());
    let mapped = map_runtime_event(&event).expect("legacy adapter should expose stream delta");
    assert_eq!(mapped.name, "streaming:delta");
}
