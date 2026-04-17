use app_lib::runtime::query_engine::QueryEngine;

#[test]
fn review_session_state_b2_total_usage_initially_zero() {
    let engine = QueryEngine::new();

    let usage = engine.get_total_usage();
    assert_eq!(usage.tokens_in, 0);
    assert_eq!(usage.tokens_out, 0);
}

#[test]
fn review_session_state_b2_total_usage_accumulates_single_update() {
    let engine = QueryEngine::new();

    engine.accumulate_usage(11, 7);

    let usage = engine.get_total_usage();
    assert_eq!(usage.tokens_in, 11);
    assert_eq!(usage.tokens_out, 7);
}

#[test]
fn review_session_state_b2_total_usage_accumulates_multiple_updates() {
    let engine = QueryEngine::new();

    engine.accumulate_usage(3, 5);
    engine.accumulate_usage(17, 19);
    engine.accumulate_usage(0, 2);

    let usage = engine.get_total_usage();
    assert_eq!(usage.tokens_in, 20);
    assert_eq!(usage.tokens_out, 26);
}
