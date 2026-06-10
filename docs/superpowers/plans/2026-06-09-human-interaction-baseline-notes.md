# Human Interaction Baseline Notes

## Dirty Files

- `src-tauri/src/connector/im/shared/ask_coordinator.rs`
- `src-tauri/src/connector/im/shared/reply_manager.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/runtime/pending/queue_manager.rs`
- `src-tauri/src/runtime/pending/queue_manager_test.rs`

## Keep

- Existing tests that prove queued IM messages can be taken from pending queue are useful evidence.
- Existing reply-manager lifecycle tests are useful evidence.

## Replace With Shared Architecture

- Any direct AskUserQuestion pending-queue harvesting inside `ask_coordinator.rs`.
- Any reply-manager behavior that lazy-creates IM output from session credentials without run output binding.

## Implementation Rule

Do not revert these files blindly. Fold useful assertions into the new tests first, then remove duplicated logic only after replacement tests pass.
