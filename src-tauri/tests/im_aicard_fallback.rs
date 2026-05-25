//! AiCardFallbackBuffer 在不同 AI 回复 pattern 下返回正确 FallbackAction。

use app_lib::connector::im::shared::aicard_fallback::{AiCardFallbackBuffer, FallbackAction};
use std::time::Duration;

#[test]
fn first_short_chunk_with_final_returns_send_final() {
    let mut buf = AiCardFallbackBuffer::new(Duration::from_secs(60));
    match buf.observe("hello", true) {
        FallbackAction::SendFinal { text } => assert_eq!(text, "hello"),
        other => panic!("expected SendFinal, got {other:?}"),
    }
}

#[test]
fn first_chunk_without_final_returns_buffer() {
    let mut buf = AiCardFallbackBuffer::new(Duration::from_secs(60));
    match buf.observe("hello", false) {
        FallbackAction::Buffer => {}
        other => panic!("expected Buffer, got {other:?}"),
    }
}

#[test]
fn multiple_chunks_then_final_concats_correctly() {
    let mut buf = AiCardFallbackBuffer::new(Duration::from_secs(60));
    assert!(matches!(buf.observe("foo ", false), FallbackAction::Buffer));
    assert!(matches!(buf.observe("bar ", false), FallbackAction::Buffer));
    match buf.observe("baz", true) {
        FallbackAction::SendFinal { text } => assert_eq!(text, "foo bar baz"),
        other => panic!("expected SendFinal, got {other:?}"),
    }
}

#[test]
fn placeholder_after_threshold_emits_once_then_buffer() {
    let mut buf = AiCardFallbackBuffer::new(Duration::from_millis(50));
    assert!(matches!(buf.observe("a", false), FallbackAction::Buffer));
    std::thread::sleep(Duration::from_millis(200));
    match buf.observe("b", false) {
        FallbackAction::SendPlaceholder { text } => assert!(text.contains("思考")),
        other => panic!("expected SendPlaceholder, got {other:?}"),
    }
    // 第二次仍未 final + 已发过 placeholder → Buffer
    match buf.observe("c", false) {
        FallbackAction::Buffer => {}
        other => panic!("placeholder should fire only once, got {other:?}"),
    }
}

#[test]
fn final_after_placeholder_still_returns_send_final_with_complete_text() {
    let mut buf = AiCardFallbackBuffer::new(Duration::from_millis(50));
    let _ = buf.observe("a", false);
    std::thread::sleep(Duration::from_millis(200));
    let _ = buf.observe("b", false); // placeholder
    let _ = buf.observe("c", false); // buffer
    match buf.observe("d", true) {
        FallbackAction::SendFinal { text } => assert_eq!(text, "abcd"),
        other => panic!("expected SendFinal, got {other:?}"),
    }
}
