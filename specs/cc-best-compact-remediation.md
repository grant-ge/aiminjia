# cc-best Compact Remediation Plan

> Date: 2026-06-02
> Base branch: `compact-fix`
> Reference: `claude-code-best` compact pipeline, especially post-compact context recovery and dynamic context-window handling.

## Why This Document Exists

The current `compact-fix` implementation compiles and the main Rust regression suite passes, but code review found several places where the behavior does not yet match the cc-best semantics recorded in `specs/replication-fix-plan.md`.

This document is the repair contract for the follow-up patch. It narrows the remaining work to behavior that affects production correctness, not cosmetic cleanup.

## Design Decisions

### 1. `CLAUDE.md` Means Project Instructions Semantics

`claude-code-best` names the recovered project instruction source `CLAUDE.md`. Lotus-app has an existing product decision to load only workspace `AGENTS.md` through `runtime/agents_md.rs`; that module explicitly does not load literal `CLAUDE.md` or `.claude/` paths.

For this repair:

- Do not reintroduce literal `CLAUDE.md` path loading.
- Treat loaded `AGENTS.md` content as the lotus-app project-instruction equivalent of cc-best `CLAUDE.md`.
- Rename new compact variables and tests away from `claude_md_*` where practical, so code does not claim to load literal `CLAUDE.md`.
- Keep the scope to this single project-instruction item. Do not add file-cache, Skill, or MCP reinjection.

If product later wants literal `CLAUDE.md` compatibility, that must be a separate prompt-architecture decision because it reverses an existing explicit loader constraint.

### 2. Reinjection Must Be a Real System Segment

Current code inserts a normal JSON message:

```json
{ "role": "system", "subtype": "claude_md_reinjection", "content": "..." }
```

This is not sufficient. The Anthropic serializer filters `role=system` messages from the normal message list and uses only the first system message as the top-level system field. A post-compact system message in the middle of history can therefore disappear before the provider request is sent.

The fix is to carry post-compact project instructions as an explicit non-cached system segment in the LLM step input:

```rust
SystemPromptSegment {
    text: project_instruction_text,
    cache: false,
}
```

This preserves the plan constraint: system segment, no `cache_control`.

### 3. Compact Summary Must Use Turn-Resolved Routing Settings

`LlmCompactSummaryClient` currently stores an `AppSettings::default()` snapshot. That can diverge from the turn's resolved Lotus model and gateway mode.

The compact summary request must use settings derived from the current turn's `ResolvedLlmSettings`, the same way the normal LLM step builds gateway settings. The compact client remains stateless with respect to conversation data: it may receive per-call settings, but it must not query conversation state internally.

`conversation_id` must not be used as Lotus sticky-routing input for the compact summary call. It is allowed only for trace/log correlation.

### 4. Dynamic Context Window Must Drive Auto-Compact Threshold

`AutoCompactConfig::with_context_window()` exists, but the production driver still creates `PreprocessConfig::default()`, which uses the conservative 64K window.

The fix is to compute the context window from the turn's cloud model or settings override before preprocess, then construct the auto-compact config from that window.

### 5. Boundary UI Is a Secondary Repair

The immediate production correctness work is backend behavior. The frontend boundary bar can remain event-backed for this patch, but a later patch should make boundary display durable and positionally accurate by reconstructing from `compact_boundaries.jsonl` or by persisting/rendering a real boundary marker.

2026-06-02 follow-up: the durable boundary marker path is now part of the cc-best alignment repair. A successful compact must persist both:

- `compact_boundaries.jsonl`: metadata/index for tail anchoring and token accounting.
- `messages.jsonl`: visible transcript artifacts, specifically `role=system, subtype=compact_boundary` and the `isCompactSummary=true` summary message.

When rebuilding model context after a sidecar boundary exists, persisted compact artifacts are treated as transcript/UI artifacts and the model-facing context is rebuilt from the sidecar summary plus the anchored tail. This avoids duplicate summary injection while preserving durable UI state.

## Required Code Changes

### Backend

1. Replace `PreprocessConfig.claude_md_content` with project-instruction naming, or add a compatibility alias only inside tests.
2. Remove `reinject_claude_md_content()` as a message-list mutation path, or stop using it in production.
3. Add a preprocess output field that returns post-compact system segments or project-instruction reinjection text.
4. Extend `LlmStepInput` and the Tauri executor path so extra system segments are appended to `system_prompt_segments(&input.system_message)`.
5. Build compact summary gateway settings from the current turn settings instead of `AppSettings::default()`.
6. Pass compact summary trace/run identifiers without passing `conversation_id` as provider sticky-routing state.
7. Build `PreprocessConfig.auto_compact` with the resolved context window in both normal and prompt-too-long recovery paths.
8. Fix `preserved_segment.preserved_token_count` so it does not divide the existing token estimate a second time.
9. Load production history without the pre-compact 120K character trim so auto-compact can inspect real context pressure.
10. Persist compact transcript artifacts to `messages.jsonl` after successful compaction, while keeping `compact_boundaries.jsonl` as the metadata/index file.

### Tests

Add or update tests before production code changes:

1. `plan_r2_reinjection_test.rs`
   - Assert post-compact project instructions are returned as a system segment, not inserted as a normal message.
   - Assert the segment has `cache=false`.
   - Assert normal messages do not contain `subtype=claude_md_reinjection`.

2. `review_compact_anthropic_protocol_test.rs`
   - Static regression: compact reinjection must not create mid-history `role=system` messages.
   - Static regression: compact summary call must not pass `Some(conversation_id)` to gateway sticky-routing.

3. `plan_r1_compact_summary_client_test.rs` or a focused new test
   - Assert compact summary uses per-call or turn-resolved settings, not a stored `AppSettings::default()` snapshot.

4. `plan_r3_boundary_test.rs`
   - Assert `preserved_token_count` equals `estimate_tokens_from_json(tail)` rather than one quarter of that value.

5. Existing regression commands must still pass:

```bash
cd src-tauri
cargo check --lib
cargo test --test review_autocompact_constraints_test -- --nocapture
cargo test --test plan_r1_compact_summary_client_test -- --nocapture
cargo test --test plan_r2_reinjection_test -- --nocapture
cargo test --test plan_r3_boundary_test -- --nocapture
cargo test --test review_compact_anthropic_protocol_test -- --nocapture
cargo test review_ --tests --no-fail-fast
```

## Out Of Scope

- Literal `CLAUDE.md` file loading.
- `.claude/CLAUDE.md`, `.claude/rules/*.md`, parent-directory traversal, or user-global instruction files.
- File read cache reinjection.
- Skill or MCP rediscovery reinjection.
- Session Memory compact.
- Reworking the frontend boundary bar persistence model.

## Acceptance Criteria

- Post-compact project instructions are available to the next main LLM request through non-cached system segments.
- No compact reinjection content is carried as a normal mid-history `role=system` message.
- Compact summary requests use turn-resolved model/gateway settings.
- Compact summary requests do not use conversation sticky-routing as state.
- Auto-compact threshold uses the resolved context window instead of the default 64K fallback.
- Production history loading preserves over-budget history and message ids before auto-compact runs.
- Successful compact writes both the sidecar boundary record and visible transcript artifacts.
- Existing compact and review regression suites remain green.
