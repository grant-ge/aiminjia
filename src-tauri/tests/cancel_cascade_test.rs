use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::state::TurnState;

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn make_turn(session: &str, run: &str, token: CancellationToken) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(session.to_string());
    TurnState::new(mapping, RunId::new(run), "test input".into())
        .with_cancellation(token)
}

// ---------------------------------------------------------------------------
// Test 1: with_cancellation(parent) — parent.cancel() propagates to turn
// ---------------------------------------------------------------------------

#[test]
fn turn_cancellation_propagates_from_parent() {
    let parent = CancellationToken::new();
    let turn = make_turn("sess-1", "run-1", parent.child_token());

    assert!(!turn.cancellation().is_cancelled(), "turn should start uncancelled");

    parent.cancel();

    assert!(
        turn.cancellation().is_cancelled(),
        "cancelling parent should cascade to turn's token"
    );
}

// ---------------------------------------------------------------------------
// Test 2: build_execution_context() cancellation is turn's child
//   - parent → turn → ctx (forward cascade)
//   - ctx cancel does NOT propagate back to turn
// ---------------------------------------------------------------------------

#[test]
fn execution_context_cancellation_is_child_of_turn() {
    let parent = CancellationToken::new();
    let turn = make_turn("sess-2", "run-2", parent.child_token());
    let ctx = turn.build_execution_context("call-2");

    assert!(!ctx.cancellation.is_cancelled());

    // Forward: parent cancel → ctx should be cancelled too
    parent.cancel();

    assert!(
        ctx.cancellation.is_cancelled(),
        "parent cancel must cascade through turn all the way to ctx"
    );
}

#[test]
fn execution_context_cancel_does_not_reverse_propagate_to_turn() {
    let parent = CancellationToken::new();
    let turn = make_turn("sess-2b", "run-2b", parent.child_token());
    let ctx = turn.build_execution_context("call-2b");

    // Cancel only the tool-call level token
    ctx.cancellation.cancel();

    assert!(ctx.cancellation.is_cancelled(), "ctx token should be cancelled");
    assert!(
        !turn.cancellation().is_cancelled(),
        "cancelling ctx must NOT propagate back to turn"
    );
    assert!(
        !parent.is_cancelled(),
        "cancelling ctx must NOT propagate back to parent session"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Three-level cascade — session → turn → tool_call
// ---------------------------------------------------------------------------

#[test]
fn three_level_cascade_session_cancel_reaches_tool_call() {
    let session_token = CancellationToken::new();
    let turn = make_turn("sess-3", "run-3", session_token.child_token());
    let ctx = turn.build_execution_context("call-3");

    assert!(!session_token.is_cancelled());
    assert!(!turn.cancellation().is_cancelled());
    assert!(!ctx.cancellation.is_cancelled());

    // Cancel at the session level
    session_token.cancel();

    assert!(session_token.is_cancelled(), "session token should be cancelled");
    assert!(
        turn.cancellation().is_cancelled(),
        "turn must be cancelled after session cancel"
    );
    assert!(
        ctx.cancellation.is_cancelled(),
        "tool call ctx must be cancelled after session cancel"
    );
}

#[test]
fn three_level_cascade_tool_call_cancel_isolated() {
    let session_token = CancellationToken::new();
    let turn = make_turn("sess-3b", "run-3b", session_token.child_token());
    let ctx = turn.build_execution_context("call-3b");

    // Cancel only the deepest level
    ctx.cancellation.cancel();

    assert!(ctx.cancellation.is_cancelled());
    assert!(
        !turn.cancellation().is_cancelled(),
        "tool call cancel must NOT affect turn"
    );
    assert!(
        !session_token.is_cancelled(),
        "tool call cancel must NOT affect session"
    );
}

#[test]
fn three_level_cascade_turn_cancel_does_not_affect_session() {
    let session_token = CancellationToken::new();
    let turn = make_turn("sess-3c", "run-3c", session_token.child_token());
    let ctx = turn.build_execution_context("call-3c");

    // Cancel at the turn level
    turn.cancellation().cancel();

    assert!(
        turn.cancellation().is_cancelled(),
        "turn token should be cancelled"
    );
    assert!(
        ctx.cancellation.is_cancelled(),
        "tool call ctx must be cancelled when turn is cancelled"
    );
    assert!(
        !session_token.is_cancelled(),
        "turn cancel must NOT propagate back to session"
    );
}

// ---------------------------------------------------------------------------
// Test 4: Multiple tool calls from one turn are independent siblings
// ---------------------------------------------------------------------------

#[test]
fn sibling_tool_calls_are_independent() {
    let session_token = CancellationToken::new();
    let turn = make_turn("sess-4", "run-4", session_token.child_token());

    let ctx_a = turn.build_execution_context("call-4a");
    let ctx_b = turn.build_execution_context("call-4b");

    // Cancelling call-4a should not affect call-4b
    ctx_a.cancellation.cancel();

    assert!(ctx_a.cancellation.is_cancelled());
    assert!(
        !ctx_b.cancellation.is_cancelled(),
        "cancelling one tool call must not affect sibling tool calls"
    );
    assert!(
        !turn.cancellation().is_cancelled(),
        "cancelling one tool call must not affect the parent turn"
    );
}
