# Human Interaction Priority Baseline

## Preserve

- Existing `SuspendedForHuman` run-registry semantics.
- Existing single permission request API compatibility.
- Existing `/approve` and `/answer` command parsing tests.
- Existing run-scoped IM output binding work.

## Replace

- `IMAskCoordinator` single `session_id -> PendingAsk` slot.
- Any permission reply path where LLM judge can answer in prose without resolving code state.
- Any late AskUserQuestion path where a queued IM message waits for the next user message before draining.
- Any App/IM divergence where one side shows a permission card and the other side receives unrelated final output.

## Manual Scenarios To Re-test

- User says `问我三个问题` while permission is pending.
- User says `好的，先拒绝吧` while permission is pending.
- User sends `好了没啊` before AskUserQuestion card arrives.
- Two permission asks arrive for the same run and same directory.
