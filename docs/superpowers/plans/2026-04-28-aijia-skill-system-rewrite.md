# AIjia SKILL.md System Rewrite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Completely replace AIjia's legacy stateful skill/workflow system with a Claude Code-aligned, disk-based SKILL.md system loaded from `~/.renlijia/skills/` and per-user `~/.renlijia/users/{scope}/skills/`.

**Architecture:** Delete legacy `plugin.toml` / `workflow.toml` / `switch_skill` / `SkillSessionStore` / precompute pipelines first, then rebuild skills as stateless SKILL.md packages. Runtime owns a disk-backed `SkillRegistry`, injects a budgeted skill catalog into dynamic context, and exposes one `load_skill` RuntimeTool supporting inline and fork modes. Frontend no longer passes `selectedSkillId`; skill selection becomes prompt text / catalog-driven model behavior.

**Tech Stack:** Rust (Tauri 2.x backend, tokio, serde, serde_yaml, shell-words), React/TypeScript (frontend cleanup), Vitest, Cargo integration tests.

---

## Scope and Safety Notes

This is a breaking migration. No backward compatibility shims are allowed.

Required outcomes:

- Runtime does **not** scan `src-tauri/plugins/`.
- Runtime does **not** parse `plugin.toml` or `workflow.toml`.
- Runtime does **not** register `DailyAssistantSkill` or any other builtin Rust skill.
- Runtime does **not** expose `switch_skill`.
- Runtime does **not** persist active skill state.
- Frontend does **not** send `selectedSkillId` / `selectedSkillLabel` to backend.
- `load_skill` loads only SKILL.md-defined skills from:
  1. `~/.renlijia/users/t_{tenant}__u_{user}/skills/`
  2. `~/.renlijia/skills/`

Broken intermediate commits are acceptable because this is a single big-bang branch. Still, every task must leave a locally testable checkpoint.

---

## File Structure Map

### Create

| Path | Responsibility |
|---|---|
| `src-tauri/src/plugin/skill/mod.rs` | New SKILL.md subsystem module root |
| `src-tauri/src/plugin/skill/types.rs` | New stateless `Skill`, `SkillFrontmatter`, `SkillCatalogEntry`, errors |
| `src-tauri/src/plugin/skill/frontmatter.rs` | Parse `SKILL.md` frontmatter + body |
| `src-tauri/src/plugin/skill/loader.rs` | Scan user/global skill dirs, precedence, reload |
| `src-tauri/src/plugin/skill/registry.rs` | Disk-backed `SkillRegistry` replacing legacy skill registry pieces |
| `src-tauri/src/plugin/skill/substitution.rs` | `$ARGUMENTS`, `${AIJIA_SKILL_DIR}`, shell blocks |
| `src-tauri/src/plugin/skill/catalog_prompt.rs` | 1% context budget + 250 char catalog formatting |
| `src-tauri/src/plugin/skill/invoked.rs` | `sent_skill_names` and `invoked_skills` in-memory tracking |
| `src-tauri/tests/skill_md_frontmatter_test.rs` | Frontmatter parsing and validation |
| `src-tauri/tests/skill_md_loader_test.rs` | Directory scan, precedence, invalid-skill skip |
| `src-tauri/tests/skill_md_substitution_test.rs` | Variable substitution and shell block tests |
| `src-tauri/tests/skill_md_catalog_test.rs` | Catalog budget + incremental listing tests |
| `src-tauri/tests/load_skill_skill_md_test.rs` | End-to-end `load_skill` tests |

### Modify

| Path | Change |
|---|---|
| `src-tauri/Cargo.toml` | Add `serde_yaml`, `shell-words` |
| `src-tauri/src/plugin/mod.rs` | Export new `plugin::skill`; stop exporting legacy skill modules |
| `src-tauri/src/plugin/registry.rs` | Remove legacy skill registry/types/factory branches; keep tool registry only |
| `src-tauri/src/runtime/tools/builtin/load_skill.rs` | Rewrite to use new registry, `args`, inline/fork execution |
| `src-tauri/src/runtime/tools/builtin/mod.rs` | Keep `load_skill`, remove `switch_skill` module |
| `src-tauri/src/runtime/tools/catalog.rs` | Remove `switch_skill`; keep/add `load_skill` schema with `args` |
| `src-tauri/src/runtime/chat/context_builder.rs` | Remove `precompute_result`; inject skill catalog only |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | Remove selected skill metadata, precompute, runtime patch; add catalog attachment state |
| `src-tauri/src/runtime/session_runtime.rs` | Remove `SkillSessionStore` field and builder plumbing |
| `src-tauri/src/runtime/agent/worker_runtime.rs` | Remove skill runtime patch; support forked load_skill child runs |
| `src-tauri/src/runtime/query_engine.rs` | Remove `extract_skill_runtime_patch` |
| `src-tauri/src/transport/tauri_commands/chat.rs` | Remove selected skill branch/params; construct new SkillRegistry from disk roots |
| `src-tauri/src/commands/chat.rs` | Remove selected skill IPC params |
| `src-tauri/src/storage/aijia_home.rs` | Keep `skills_dir()` as global skill dir; ensure directory creation remains |
| `src-tauri/src/commands/skill_management.rs` | Remove legacy plugin scaffolding; align with SKILL.md import/listing |
| `src-tauri/src/commands/skill_smith/*` | Remove or rewrite validators to SKILL.md-only |
| `src/lib/tauri.ts` | Remove `selectedSkillId` / `selectedSkillLabel` from `sendMessage` |
| `src/hooks/useChat.ts` | Remove slash-to-selectedSkillId parsing and selected skill state plumbing |
| `src/components/chat-scene/ChatBottomArea.tsx` | Remove selected skill command forwarding/popover wiring |
| `src/components/chat/WelcomeScreen.tsx` | Ensure cards only send plain prompt text |
| `src/components/chat/SlashCommandPopover.tsx` | Delete or disconnect from chat composer |
| `src/stores/chatStore.ts` | Remove `selectedSkillCommands` state/actions |
| `src/hooks/useStreaming.ts` | Remove `HIDDEN_TOOLS = ['switch_skill']` logic |

### Delete

| Path | Reason |
|---|---|
| `src-tauri/src/plugin/manifest.rs` | Legacy `plugin.toml` / `workflow.toml` parser |
| `src-tauri/src/plugin/declarative_skill.rs` | Legacy multi-step declarative workflow skill |
| `src-tauri/src/plugin/builtin/skills/daily_assistant.rs` | Hardcoded builtin skill; default skill concept removed |
| `src-tauri/src/plugin/builtin/skills/mod.rs` | Legacy builtin skill module |
| `src-tauri/src/runtime/tools/builtin/switch_skill.rs` | Stateful skill switching removed |
| `src-tauri/src/runtime/chat/skill_session.rs` | Persistent active skill state removed |
| `src-tauri/src/runtime/chat/tool_round_types.rs` | Only contains `SkillRuntimePatch`; delete or empty module |
| `src-tauri/src/llm/checkpoint.rs` | Stateful workflow checkpoint extraction removed |
| `src-tauri/src/llm/orchestrator.rs` | Step state orchestration removed |
| `src-tauri/tests/plugin_workflow_audit_test.rs` | Legacy workflow-only test |
| `src-tauri/tests/review_skill_loading_test.rs` | Legacy `switch_skill` / `SkillSessionStore` test |
| `src-tauri/tests/skill_routing_llm_test.rs` | Legacy LLM-routed `switch_skill` test |
| `src/hooks/useChat.skill.test.ts` | Legacy selectedSkillId frontend test |

---

## Phase A: Delete Stateful Skill Switching and Workflow Patch Chain

### Task 1: Add dependencies for the new SKILL.md runtime

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Add YAML and shell argument dependencies**

Add to `[dependencies]` in `src-tauri/Cargo.toml`:

```toml
serde_yaml = "0.9"
shell-words = "1.1"
```

- [ ] **Step 2: Verify dependency resolution**

Run:

```bash
cd src-tauri && cargo check --quiet
```

Expected: PASS or existing unrelated compile errors. There should be no "failed to select a version" errors for `serde_yaml` or `shell-words`.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "chore(skill): add dependencies for SKILL.md runtime"
```

---

### Task 2: Remove `switch_skill` RuntimeTool registration

**Files:**
- Modify: `src-tauri/src/runtime/tools/builtin/mod.rs`
- Modify: `src-tauri/src/runtime/tools/catalog.rs`
- Modify: `src-tauri/src/plugin/registry.rs`
- Delete: `src-tauri/src/runtime/tools/builtin/switch_skill.rs`
- Test: `src-tauri/tests/builtin_runtime_registration_test.rs`

- [ ] **Step 1: Delete switch_skill-specific tests**

In `src-tauri/tests/builtin_runtime_registration_test.rs`, delete the test named:

```rust
switch_skill_routes_through_request_scoped_runtime_factory
```

Also delete any helper code used only by that test and no other test.

- [ ] **Step 2: Remove switch_skill from builtin modules**

In `src-tauri/src/runtime/tools/builtin/mod.rs`, remove:

```rust
pub mod switch_skill;
```

- [ ] **Step 3: Remove switch_skill from runtime tool catalog**

In `src-tauri/src/runtime/tools/catalog.rs`:

1. Remove the `switch_skill` entry from `build_default_catalog()`.
2. Remove `"switch_skill"` from any allowlist arrays.
3. Ensure `"load_skill"` remains listed in `DAILY_ALLOWED_TOOLS`.

- [ ] **Step 4: Remove switch_skill request-scoped factory branch**

In `src-tauri/src/plugin/registry.rs`:

1. Remove `"switch_skill"` from `REQUEST_SCOPED_RUNTIME_TOOL_NAMES`.
2. Remove the `"switch_skill" => { ... }` arm from `try_build_request_scoped_tool()`.
3. Remove imports that only exist for `SwitchSkillRuntimeTool`.

- [ ] **Step 5: Delete the file**

Run:

```bash
rm src-tauri/src/runtime/tools/builtin/switch_skill.rs
```

- [ ] **Step 6: Verify switch_skill no longer exists in runtime code**

Run:

```bash
grep -rn "switch_skill" src-tauri/src/runtime src-tauri/src/plugin src-tauri/src/transport src-tauri/tests/builtin_runtime_registration_test.rs
```

Expected: no production references. Test docs may still mention it only in deleted-plan documents, not source.

- [ ] **Step 7: Run registration test**

Run:

```bash
cd src-tauri && cargo test --test builtin_runtime_registration_test load_skill_routes_through_request_scoped_runtime_factory -- --nocapture
```

Expected: PASS. If the old test name no longer exists, run the whole file:

```bash
cd src-tauri && cargo test --test builtin_runtime_registration_test -- --nocapture
```

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/runtime/tools/builtin/mod.rs src-tauri/src/runtime/tools/catalog.rs src-tauri/src/plugin/registry.rs src-tauri/tests/builtin_runtime_registration_test.rs
git rm src-tauri/src/runtime/tools/builtin/switch_skill.rs
git commit -m "refactor(skill): remove stateful switch_skill runtime tool"
```

---

### Task 3: Remove SkillRuntimePatch hot-update chain

**Files:**
- Delete or empty: `src-tauri/src/runtime/chat/tool_round_types.rs`
- Modify: `src-tauri/src/runtime/chat/tool_result_collector.rs`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/runtime/query_engine.rs`
- Modify: `src-tauri/src/runtime/agent/worker_runtime.rs`
- Tests: `src-tauri/tests/s4_driver_loop_test.rs`, `src-tauri/tests/prompt_architecture_test.rs`

- [ ] **Step 1: Remove collector field test expectations**

Search tests for `skill_runtime_patch`:

```bash
grep -rn "skill_runtime_patch\|SkillRuntimePatch\|skill_control" src-tauri/tests src-tauri/src --exclude-dir=target
```

Delete tests that assert patch extraction. Rewrite only tests that also cover non-skill behavior.

- [ ] **Step 2: Remove `SkillRuntimePatch` type**

If `src-tauri/src/runtime/chat/tool_round_types.rs` only contains `SkillRuntimePatch`, delete the file and remove its `mod` declaration.

If other types are present, remove only:

```rust
pub struct SkillRuntimePatch { ... }
```

- [ ] **Step 3: Remove collector field**

In `src-tauri/src/runtime/chat/tool_result_collector.rs`, remove:

```rust
pub skill_runtime_patch: Option<SkillRuntimePatch>,
```

Remove extraction / assignment code that populates it.

- [ ] **Step 4: Remove query engine extraction function**

In `src-tauri/src/runtime/query_engine.rs`, delete:

```rust
fn extract_skill_runtime_patch(...)
```

Remove any call site that reads `skill_control` from tool result data.

- [ ] **Step 5: Remove chat turn hot-swap function**

In `src-tauri/src/runtime/chat/chat_turn_driver.rs`, delete:

```rust
fn apply_skill_runtime_patch(...)
```

Remove every call to it in the S4 turn loop.

- [ ] **Step 6: Remove worker hot-swap duplicate**

In `src-tauri/src/runtime/agent/worker_runtime.rs`, remove worker-specific `apply_skill_runtime_patch` and all callers.

- [ ] **Step 7: Verify no patch references remain**

Run:

```bash
grep -rn "SkillRuntimePatch\|skill_runtime_patch\|skill_control\|apply_skill_runtime_patch" src-tauri/src src-tauri/tests --exclude-dir=target
```

Expected: no source/test references.

- [ ] **Step 8: Run targeted Rust tests**

Run serially:

```bash
cd src-tauri && cargo test --test s4_driver_loop_test -- --nocapture
cd src-tauri && cargo test --test prompt_architecture_test -- --nocapture
```

Expected: PASS after removing patch-specific assertions.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/runtime/chat src-tauri/src/runtime/query_engine.rs src-tauri/src/runtime/agent/worker_runtime.rs src-tauri/tests/s4_driver_loop_test.rs src-tauri/tests/prompt_architecture_test.rs
git commit -m "refactor(skill): remove skill runtime patch hot-swap chain"
```

---

### Task 4: Remove SkillSessionStore from session and transport

**Files:**
- Delete: `src-tauri/src/runtime/chat/skill_session.rs`
- Modify: `src-tauri/src/runtime/session_runtime.rs`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Modify: `src-tauri/src/commands/chat.rs`
- Modify: `src-tauri/src/plugin/registry.rs`
- Delete: `src-tauri/tests/review_skill_loading_test.rs`
- Delete: `src-tauri/tests/skill_routing_llm_test.rs`

- [ ] **Step 1: Delete legacy skill session tests**

Run:

```bash
git rm src-tauri/tests/review_skill_loading_test.rs src-tauri/tests/skill_routing_llm_test.rs
```

- [ ] **Step 2: Remove ChatTurnRequest selected skill fields**

In `src-tauri/src/runtime/chat/chat_turn_driver.rs`, remove fields:

```rust
pub selected_skill_id: Option<String>,
pub selected_skill_label: Option<String>,
```

Also remove their default initialization and any references in `build_user_content_json`.

- [ ] **Step 3: Remove selected skill metadata from user content JSON**

Delete helper code that injects:

```json
"selected_skill_id"
"selected_skill_label"
```

into user message content. Existing tests named like `build_user_content_json_includes_selected_skill_metadata` should be deleted or rewritten to assert no such keys exist.

- [ ] **Step 4: Remove SkillSessionStore from SessionRuntime**

In `src-tauri/src/runtime/session_runtime.rs`, remove:

```rust
skill_sessions: Arc<SkillSessionStore>
.with_skill_sessions(...)
```

and all builder fields/methods that exist only for `SkillSessionStore`.

- [ ] **Step 5: Remove transport selected_skill branch**

In `src-tauri/src/transport/tauri_commands/chat.rs`, delete function or branch equivalent to:

```rust
if let Some(selected_skill_id) = request.selected_skill_id.as_deref() {
    skill_sessions.switch_skill(...)
}
```

The turn config should now use the minimal base prompt and tool allowlist, not `SkillSessionStore::resolve_turn_context`.

- [ ] **Step 6: Remove send_message IPC params**

In `src-tauri/src/commands/chat.rs` and `src-tauri/src/transport/tauri_commands/chat.rs`, remove command parameters:

```rust
selected_skill_id: Option<String>,
selected_skill_label: Option<String>,
```

Do not keep unused `_selected_skill_id` parameters.

- [ ] **Step 7: Remove RequestScopedRuntimeDeps skill_sessions field**

In `src-tauri/src/plugin/registry.rs`, remove:

```rust
pub skill_sessions: Option<Arc<SkillSessionStore>>,
```

Update every struct literal building `RequestScopedRuntimeDeps`.

- [ ] **Step 8: Delete skill_session module**

Remove module declaration from `runtime/chat/mod.rs` and delete:

```bash
git rm src-tauri/src/runtime/chat/skill_session.rs
```

- [ ] **Step 9: Verify no selected skill/session references remain**

Run:

```bash
grep -rn "SkillSessionStore\|skill_sessions\|selected_skill_id\|selectedSkillId\|selected_skill_label\|selectedSkillLabel" src-tauri/src src-tauri/tests src --exclude-dir=target
```

Expected: no production references. Frontend references will be removed in Phase E; if Phase E not done yet, document remaining `src/` references.

- [ ] **Step 10: Run core Rust tests**

Run serially:

```bash
cd src-tauri && cargo test --test runtime_dependencies_production_wiring_test -- --nocapture
cd src-tauri && cargo test --test builtin_runtime_registration_test -- --nocapture
```

Expected: PASS after updating fixtures.

- [ ] **Step 11: Commit**

```bash
git add src-tauri/src/runtime src-tauri/src/transport src-tauri/src/commands src-tauri/src/plugin/registry.rs src-tauri/tests
git commit -m "refactor(skill): remove persistent skill session routing"
```

---

## Phase B: Delete Legacy Skill Format and Workflow Pipeline

### Task 5: Remove plugin.toml / workflow.toml parsing code

**Files:**
- Delete: `src-tauri/src/plugin/manifest.rs`
- Delete: `src-tauri/src/plugin/declarative_skill.rs`
- Modify: `src-tauri/src/plugin/mod.rs`
- Modify: `src-tauri/src/plugin/skill_trait.rs`
- Delete: `src-tauri/tests/plugin_workflow_audit_test.rs`

- [ ] **Step 1: Delete legacy workflow audit test**

Run:

```bash
git rm src-tauri/tests/plugin_workflow_audit_test.rs
```

- [ ] **Step 2: Delete legacy implementation files**

Run:

```bash
git rm src-tauri/src/plugin/manifest.rs src-tauri/src/plugin/declarative_skill.rs
```

- [ ] **Step 3: Remove module exports**

In `src-tauri/src/plugin/mod.rs`, remove:

```rust
pub mod manifest;
pub mod declarative_skill;
```

and remove any re-exports for `DeclarativeSkill`, `PluginManifest`, `WorkflowManifest`, etc.

- [ ] **Step 4: Simplify `skill_trait.rs` temporarily**

Replace legacy `SkillState`, `WorkflowDefinition`, `WorkflowStep`, `StepAction`, and the old `Skill` trait with a temporary minimal trait only if needed by remaining code:

```rust
pub trait LegacySkillRemoved {}
```

Prefer deleting `skill_trait.rs` entirely if no production code needs it after Task 4.

- [ ] **Step 5: Verify old format strings disappear**

Run:

```bash
grep -rn "plugin.toml\|workflow.toml\|WorkflowManifest\|DeclarativeSkill\|WorkflowDefinition\|prompts/step" src-tauri/src src-tauri/tests --exclude-dir=target
```

Expected: only `commands/skill_management.rs` and `commands/skill_smith/*` may remain until Task 6. No runtime loading path should remain.

- [ ] **Step 6: Compile check**

Run:

```bash
cd src-tauri && cargo check --quiet
```

Expected: FAIL only on skill-management / skill-smith references if Task 6 not yet complete. Record errors before proceeding.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/plugin src-tauri/tests
git commit -m "refactor(skill): remove legacy plugin manifest skill format"
```

---

### Task 6: Remove builtin daily-assistant skill

**Files:**
- Delete: `src-tauri/src/plugin/builtin/skills/daily_assistant.rs`
- Delete: `src-tauri/src/plugin/builtin/skills/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Create: `src-tauri/src/runtime/chat/base_prompt.rs`
- Modify: `src-tauri/src/runtime/chat/mod.rs`
- Tests: `src-tauri/tests/prompt_architecture_test.rs`

- [ ] **Step 1: Create minimal base prompt module**

Create `src-tauri/src/runtime/chat/base_prompt.rs`:

```rust
pub const DAILY_BASE_PROMPT: &str = r#"你是 AI小家，一个面向企业工作场景的 AI 助手。

遵循以下规则：
- 优先使用可用工具完成用户请求。
- 当用户需求匹配可用技能目录时，调用 load_skill 加载专项技能指令。
- 不要假装已经读取文件、运行脚本或调用外部服务。
"#;
```

- [ ] **Step 2: Export base prompt module**

In `src-tauri/src/runtime/chat/mod.rs`, add:

```rust
pub mod base_prompt;
```

- [ ] **Step 3: Delete builtin skill module files**

Run:

```bash
git rm src-tauri/src/plugin/builtin/skills/daily_assistant.rs src-tauri/src/plugin/builtin/skills/mod.rs
```

- [ ] **Step 4: Remove register_builtin_skills call**

In `src-tauri/src/lib.rs`, remove any block calling:

```rust
plugin::builtin::skills::register_builtin_skills(...)
```

and remove related imports/logging.

- [ ] **Step 5: Wire base prompt into turn config**

In the code that builds default `TurnConfig.system_prompt`, replace dependency on `SkillSessionStore` / default skill with:

```rust
crate::runtime::chat::base_prompt::DAILY_BASE_PROMPT.to_string()
```

- [ ] **Step 6: Update prompt tests**

In `src-tauri/tests/prompt_architecture_test.rs`, assert that default system prompt contains:

```rust
"你是 AI小家"
```

and does not contain:

```rust
"daily-assistant"
"switch_skill"
```

- [ ] **Step 7: Run prompt tests**

Run:

```bash
cd src-tauri && cargo test --test prompt_architecture_test -- --nocapture
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/runtime/chat src-tauri/src/lib.rs src-tauri/tests/prompt_architecture_test.rs
git commit -m "refactor(skill): replace builtin daily skill with base prompt"
```

---

### Task 7: Remove precompute / checkpoint / is_analysis workflow pipeline

**Files:**
- Delete: `src-tauri/src/llm/checkpoint.rs`
- Delete: `src-tauri/src/llm/orchestrator.rs`
- Modify: `src-tauri/src/llm/mod.rs`
- Modify: `src-tauri/src/llm/analysis_context.rs`
- Modify: `src-tauri/src/llm/tool_executor/python.rs`
- Modify: `src-tauri/src/llm/tool_executor/file_load.rs`
- Modify: `src-tauri/src/runtime/chat/context_builder.rs`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Modify: `src-tauri/src/runtime/agent/python_recovery.rs`
- Tests: `src-tauri/tests/s4_driver_loop_test.rs`, `src-tauri/tests/python_recovery_input_test.rs`

- [ ] **Step 1: Delete workflow-only modules**

Run:

```bash
git rm src-tauri/src/llm/checkpoint.rs src-tauri/src/llm/orchestrator.rs
```

Remove their `mod` declarations from `src-tauri/src/llm/mod.rs`.

- [ ] **Step 2: Remove `current_step` from AnalysisContext**

In `src-tauri/src/llm/analysis_context.rs`, remove `current_step` field and methods that only advance workflow steps.

Keep general analysis context fields that are used by non-workflow Python execution.

- [ ] **Step 3: Remove is_analysis branches in Python executor**

In `src-tauri/src/llm/tool_executor/python.rs`, delete code equivalent to:

```rust
if is_analysis {
    let step = orchestrator::get_step_state(...)
    ...
}
```

There should be one general Python execution path.

- [ ] **Step 4: Remove workflow step file-load behavior**

In `src-tauri/src/llm/tool_executor/file_load.rs`, remove `get_step_state` guards and any branch that behaves differently inside a workflow step.

- [ ] **Step 5: Remove precompute_result dynamic context parameter**

In `src-tauri/src/runtime/chat/context_builder.rs`, change signature from:

```rust
pub fn build_iteration_context(
    core_memory: &str,
    project_memory: &str,
    workspace_context: &str,
    file_context: &str,
    analysis_notes: &str,
    precompute_result: Option<&str>,
    connector_context: Option<&str>,
    analysis_ctx_prompt: Option<&str>,
    skill_catalog: &str,
) -> String
```

to:

```rust
pub fn build_iteration_context(
    core_memory: &str,
    project_memory: &str,
    workspace_context: &str,
    file_context: &str,
    analysis_notes: &str,
    connector_context: Option<&str>,
    analysis_ctx_prompt: Option<&str>,
    skill_catalog: &str,
) -> String
```

Delete the `[precompute_result]...[/precompute_result]` block.

- [ ] **Step 6: Remove run_precompute from executor trait**

In `src-tauri/src/runtime/chat/chat_turn_driver.rs`, remove `run_precompute` from `RuntimeLlmExecutor` and every call that stores `precompute_result`.

- [ ] **Step 7: Remove precompute stub in Tauri executor**

In `src-tauri/src/transport/tauri_commands/chat.rs`, delete the `run_precompute` stub implementation.

- [ ] **Step 8: Remove recovery precompute fields**

In `src-tauri/src/runtime/agent/python_recovery.rs`, remove:

```rust
precompute_cache_paths: Vec<String>
```

from structs and tests.

- [ ] **Step 9: Verify no workflow pipeline strings remain**

Run:

```bash
grep -rn "precompute\|checkpoint\|is_analysis\|get_step_state\|current_step\|\[precompute_result\]" src-tauri/src src-tauri/tests --exclude-dir=target
```

Expected: no production references. Docs may still contain legacy history.

- [ ] **Step 10: Run tests**

Run serially:

```bash
cd src-tauri && cargo test --test s4_driver_loop_test -- --nocapture
cd src-tauri && cargo test --test python_recovery_input_test -- --nocapture
```

Expected: PASS after updating assertions.

- [ ] **Step 11: Commit**

```bash
git add src-tauri/src/llm src-tauri/src/runtime src-tauri/src/transport src-tauri/tests
git commit -m "refactor(skill): remove stateful workflow precompute pipeline"
```

---

### Task 8: Rewrite skill management and skill-smith commands to SKILL.md-only

**Files:**
- Modify: `src-tauri/src/commands/skill_management.rs`
- Modify/Delete: `src-tauri/src/commands/skill_smith/validation.rs`
- Modify/Delete: `src-tauri/src/commands/skill_smith/dry_run.rs`
- Modify: `src-tauri/src/plugin/builtin/tools/skill_smith_validate.rs`
- Modify: `src-tauri/src/plugin/builtin/tools/skill_smith_write_file.rs`
- Modify: `src-tauri/src/plugin/builtin/tools/skill_smith_install.rs`

- [ ] **Step 1: Replace generated skill template**

In `src-tauri/src/commands/skill_management.rs`, update `init_skill_template` so it creates only:

```
<skill-id>/SKILL.md
<skill-id>/scripts/.gitkeep
<skill-id>/references/.gitkeep
<skill-id>/assets/.gitkeep
```

It must not create `plugin.toml`, `workflow.toml`, or `prompts/step0.md`.

Use SKILL.md content:

```markdown
---
name: <skill-id>
description: 描述这个技能何时应该被使用。
---

# <skill-id>

说明如何完成这个技能支持的任务。

可用资源：
- `${AIJIA_SKILL_DIR}/scripts/`
- `${AIJIA_SKILL_DIR}/references/`
- `${AIJIA_SKILL_DIR}/assets/`
```

- [ ] **Step 2: Rewrite validation to SKILL.md-only**

In `src-tauri/src/commands/skill_smith/validation.rs`, remove validation of `plugin.toml`, `workflow.toml`, and `prompts/step*.md`.

Validation must check:

1. `SKILL.md` exists.
2. YAML frontmatter parses.
3. `name` and `description` are non-empty.
4. Directory name is valid skill-id.

- [ ] **Step 3: Rewrite dry run to SKILL.md-only**

In `src-tauri/src/commands/skill_smith/dry_run.rs`, remove plugin/workflow checks and call the new `plugin::skill::frontmatter::parse_skill_md` helper.

- [ ] **Step 4: Update skill-smith tool descriptions**

In the three `src-tauri/src/plugin/builtin/tools/skill_smith_*.rs` files, replace mentions of:

```text
plugin.toml
workflow.toml
prompts/step0.md
```

with:

```text
SKILL.md
scripts/
references/
assets/
```

- [ ] **Step 5: Verify legacy scaffold strings are gone**

Run:

```bash
grep -rn "plugin.toml\|workflow.toml\|prompts/step" src-tauri/src/commands src-tauri/src/plugin/builtin/tools --exclude-dir=target
```

Expected: no references.

- [ ] **Step 6: Run skill management tests**

Run:

```bash
cd src-tauri && cargo test skill_management -- --nocapture
```

If no test target exists, run:

```bash
cd src-tauri && cargo test --lib skill_smith -- --nocapture
```

Expected: PASS or no matching tests. Compile must pass.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands src-tauri/src/plugin/builtin/tools
git commit -m "refactor(skill): make skill management SKILL.md-only"
```

---

## Phase C: Build SKILL.md Loader and Registry

### Task 9: Create SKILL.md frontmatter parser

**Files:**
- Create: `src-tauri/src/plugin/skill/mod.rs`
- Create: `src-tauri/src/plugin/skill/types.rs`
- Create: `src-tauri/src/plugin/skill/frontmatter.rs`
- Modify: `src-tauri/src/plugin/mod.rs`
- Test: `src-tauri/tests/skill_md_frontmatter_test.rs`

- [ ] **Step 1: Write frontmatter tests**

Create `src-tauri/tests/skill_md_frontmatter_test.rs`:

```rust
use app_lib::plugin::skill::frontmatter::parse_skill_md;

#[test]
fn parses_required_skill_md_frontmatter_and_body() {
    let input = r#"---
name: salary-query
description: 薪酬查询
metadata:
  label: 薪酬市场数据查询助手
---

# Body
Use `${AIJIA_SKILL_DIR}/scripts/call.py`.
"#;

    let parsed = parse_skill_md(input).expect("valid SKILL.md should parse");
    assert_eq!(parsed.frontmatter.name, "salary-query");
    assert_eq!(parsed.frontmatter.description, "薪酬查询");
    assert_eq!(
        parsed.frontmatter.metadata.label.as_deref(),
        Some("薪酬市场数据查询助手")
    );
    assert!(parsed.body.contains("# Body"));
}

#[test]
fn rejects_missing_frontmatter() {
    let err = parse_skill_md("# Body only").unwrap_err().to_string();
    assert!(err.contains("frontmatter"), "unexpected error: {err}");
}

#[test]
fn rejects_missing_required_name_or_description() {
    let missing_name = "---\ndescription: x\n---\nbody";
    assert!(parse_skill_md(missing_name).unwrap_err().to_string().contains("name"));

    let missing_desc = "---\nname: x\n---\nbody";
    assert!(parse_skill_md(missing_desc).unwrap_err().to_string().contains("description"));
}

#[test]
fn accepts_all_claude_code_fields() {
    let input = r#"---
name: code-review
description: Review code
when_to_use: user asks for code review
allowed-tools:
  - read_file
  - bash
argument-hint: <path>
arguments: path severity
model: opus
effort: high
context: fork
agent: code-reviewer
user-invocable: false
disable-model-invocation: true
version: "1.0"
paths:
  - "src/**/*.rs"
hooks:
  PreToolUse:
    - command: ["echo", "hi"]
shell: bash
metadata:
  label: Code Review
unknown-field: ignored
---
body
"#;
    let parsed = parse_skill_md(input).expect("all supported fields should parse");
    assert_eq!(parsed.frontmatter.context.as_deref(), Some("fork"));
    assert_eq!(parsed.frontmatter.allowed_tools, vec!["read_file", "bash"]);
    assert_eq!(parsed.frontmatter.arguments, vec!["path", "severity"]);
    assert!(parsed.frontmatter.disable_model_invocation);
}
```

- [ ] **Step 2: Run tests and confirm failure**

Run:

```bash
cd src-tauri && cargo test --test skill_md_frontmatter_test -- --nocapture
```

Expected: FAIL because `plugin::skill::frontmatter` does not exist.

- [ ] **Step 3: Create module root**

Create `src-tauri/src/plugin/skill/mod.rs`:

```rust
pub mod frontmatter;
pub mod types;
```

In `src-tauri/src/plugin/mod.rs`, add:

```rust
pub mod skill;
```

- [ ] **Step 4: Create types**

Create `src-tauri/src/plugin/skill/types.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMetadata {
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub when_to_use: Option<String>,
    #[serde(default, deserialize_with = "crate::plugin::skill::frontmatter::deserialize_string_or_vec")]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub argument_hint: Option<String>,
    #[serde(default, deserialize_with = "crate::plugin::skill::frontmatter::deserialize_string_or_vec")]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default = "default_true")]
    pub user_invocable: bool,
    #[serde(default)]
    pub disable_model_invocation: bool,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub hooks: serde_yaml::Value,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub metadata: SkillMetadata,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSkillMd {
    pub frontmatter: SkillFrontmatter,
    pub body: String,
}
```

- [ ] **Step 5: Create parser**

Create `src-tauri/src/plugin/skill/frontmatter.rs`:

```rust
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Deserializer};

use super::types::{ParsedSkillMd, SkillFrontmatter};

pub fn parse_skill_md(input: &str) -> Result<ParsedSkillMd> {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let Some(rest) = input.strip_prefix("---\n") else {
        bail!("SKILL.md missing YAML frontmatter");
    };
    let Some(end) = rest.find("\n---") else {
        bail!("SKILL.md frontmatter is not closed with ---");
    };
    let yaml = &rest[..end];
    let body_start = end + "\n---".len();
    let body = rest[body_start..]
        .strip_prefix('\n')
        .unwrap_or(&rest[body_start..])
        .to_string();

    let frontmatter: SkillFrontmatter = serde_yaml::from_str(yaml)
        .with_context(|| "Failed to parse SKILL.md YAML frontmatter")?;

    if frontmatter.name.trim().is_empty() {
        bail!("SKILL.md frontmatter field 'name' is required");
    }
    if frontmatter.description.trim().is_empty() {
        bail!("SKILL.md frontmatter field 'description' is required");
    }

    Ok(ParsedSkillMd { frontmatter, body })
}

pub fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        String(String),
        Vec(Vec<String>),
    }

    let value = Option::<StringOrVec>::deserialize(deserializer)?;
    Ok(match value {
        None => Vec::new(),
        Some(StringOrVec::String(s)) => shell_words::split(&s)
            .map_err(serde::de::Error::custom)?,
        Some(StringOrVec::Vec(v)) => v,
    })
}
```

- [ ] **Step 6: Run parser tests**

Run:

```bash
cd src-tauri && cargo test --test skill_md_frontmatter_test -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/plugin/mod.rs src-tauri/src/plugin/skill src-tauri/tests/skill_md_frontmatter_test.rs
git commit -m "feat(skill): parse SKILL.md frontmatter"
```

---

### Task 10: Implement disk skill loader and precedence

**Files:**
- Create: `src-tauri/src/plugin/skill/loader.rs`
- Modify: `src-tauri/src/plugin/skill/mod.rs`
- Modify: `src-tauri/src/plugin/skill/types.rs`
- Test: `src-tauri/tests/skill_md_loader_test.rs`

- [ ] **Step 1: Write loader tests**

Create `src-tauri/tests/skill_md_loader_test.rs`:

```rust
use std::fs;

use app_lib::plugin::skill::loader::load_skill_roots;
use tempfile::TempDir;

fn write_skill(root: &std::path::Path, id: &str, desc: &str, body: &str) {
    let dir = root.join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {id}\ndescription: {desc}\n---\n\n{body}\n"),
    )
    .unwrap();
}

#[test]
fn loads_user_and_global_skills_with_user_precedence() {
    let global = TempDir::new().unwrap();
    let user = TempDir::new().unwrap();
    write_skill(global.path(), "salary-query", "global desc", "global body");
    write_skill(user.path(), "salary-query", "user desc", "user body");
    write_skill(global.path(), "biz-writing", "biz desc", "biz body");

    let skills = load_skill_roots(&[user.path().to_path_buf(), global.path().to_path_buf()]);
    let skills = skills.expect("skills should load");

    assert_eq!(skills.len(), 2);
    assert_eq!(skills.get("salary-query").unwrap().frontmatter.description, "user desc");
    assert_eq!(skills.get("biz-writing").unwrap().frontmatter.description, "biz desc");
}

#[test]
fn skips_hidden_draft_and_invalid_entries() {
    let root = TempDir::new().unwrap();
    write_skill(root.path(), "valid-skill", "valid", "body");
    write_skill(root.path(), "_draft", "draft", "body");
    fs::write(root.path().join("loose.md"), "---\nname: loose\ndescription: no\n---\n").unwrap();

    let skills = load_skill_roots(&[root.path().to_path_buf()]).unwrap();
    assert!(skills.contains_key("valid-skill"));
    assert!(!skills.contains_key("_draft"));
    assert!(!skills.contains_key("loose"));
}

#[test]
fn rejects_directory_name_that_is_not_skill_id() {
    let root = TempDir::new().unwrap();
    write_skill(root.path(), "BadSkill", "bad", "body");
    let skills = load_skill_roots(&[root.path().to_path_buf()]).unwrap();
    assert!(skills.is_empty());
}
```

- [ ] **Step 2: Run tests and confirm failure**

Run:

```bash
cd src-tauri && cargo test --test skill_md_loader_test -- --nocapture
```

Expected: FAIL because loader does not exist.

- [ ] **Step 3: Extend types**

In `src-tauri/src/plugin/skill/types.rs`, add:

```rust
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct DiskSkill {
    pub id: String,
    pub root: PathBuf,
    pub frontmatter: SkillFrontmatter,
    pub body: String,
    pub source: SkillSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSource {
    User,
    Global,
}
```

- [ ] **Step 4: Implement loader**

Create `src-tauri/src/plugin/skill/loader.rs`:

```rust
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use super::frontmatter::parse_skill_md;
use super::types::{DiskSkill, SkillSource};

pub fn is_valid_skill_id(id: &str) -> bool {
    let mut chars = id.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit() => {}
        _ => return false,
    }
    id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

pub fn load_skill_roots(roots: &[PathBuf]) -> Result<HashMap<String, DiskSkill>> {
    let mut loaded = HashMap::new();
    for (idx, root) in roots.iter().enumerate() {
        let source = if idx == 0 { SkillSource::User } else { SkillSource::Global };
        load_one_root(root, source, &mut loaded)?;
    }
    Ok(loaded)
}

fn load_one_root(
    root: &Path,
    source: SkillSource,
    loaded: &mut HashMap<String, DiskSkill>,
) -> Result<()> {
    if !root.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('_') || name.starts_with('.') || !is_valid_skill_id(name) {
            continue;
        }
        if loaded.contains_key(name) {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        let content = fs::read_to_string(&skill_md)?;
        let parsed = match parse_skill_md(&content) {
            Ok(parsed) => parsed,
            Err(err) => {
                log::error!("Failed to parse skill {} at {}: {}", name, skill_md.display(), err);
                continue;
            }
        };
        loaded.insert(
            name.to_string(),
            DiskSkill {
                id: name.to_string(),
                root: path,
                frontmatter: parsed.frontmatter,
                body: parsed.body,
                source,
            },
        );
    }
    Ok(())
}
```

- [ ] **Step 5: Export loader**

In `src-tauri/src/plugin/skill/mod.rs`, add:

```rust
pub mod loader;
```

- [ ] **Step 6: Run tests**

Run:

```bash
cd src-tauri && cargo test --test skill_md_loader_test -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/plugin/skill src-tauri/tests/skill_md_loader_test.rs
git commit -m "feat(skill): load SKILL.md directories from disk roots"
```

---

### Task 11: Implement SkillRegistry and catalog formatting

**Files:**
- Create: `src-tauri/src/plugin/skill/registry.rs`
- Create: `src-tauri/src/plugin/skill/catalog_prompt.rs`
- Modify: `src-tauri/src/plugin/skill/mod.rs`
- Modify: `src-tauri/src/plugin/skill/types.rs`
- Test: `src-tauri/tests/skill_md_catalog_test.rs`

- [ ] **Step 1: Write catalog tests**

Create `src-tauri/tests/skill_md_catalog_test.rs`:

```rust
use app_lib::plugin::skill::catalog_prompt::format_skill_catalog_with_budget;
use app_lib::plugin::skill::registry::SkillRegistry;
use app_lib::plugin::skill::types::{DiskSkill, SkillFrontmatter, SkillSource};
use std::path::PathBuf;

fn skill(id: &str, desc: &str) -> DiskSkill {
    DiskSkill {
        id: id.to_string(),
        root: PathBuf::from(format!("/tmp/{id}")),
        frontmatter: SkillFrontmatter {
            name: id.to_string(),
            description: desc.to_string(),
            ..Default::default()
        },
        body: format!("body for {id}"),
        source: SkillSource::User,
    }
}

#[test]
fn catalog_respects_budget_and_desc_cap() {
    let entries = vec![skill("salary-query", &"x".repeat(400))];
    let catalog = format_skill_catalog_with_budget(&entries, 200_000);
    assert!(catalog.contains("salary-query"));
    assert!(catalog.len() < 1_000);
}

#[test]
fn registry_tracks_sent_skill_names_incrementally() {
    let mut registry = SkillRegistry::from_skills(vec![skill("a-skill", "A"), skill("b-skill", "B")]);
    let first = registry.catalog_delta_for_agent(None, 200_000);
    assert!(first.contains("a-skill"));
    assert!(first.contains("b-skill"));

    let second = registry.catalog_delta_for_agent(None, 200_000);
    assert!(second.is_empty(), "second call should send no already-sent skills");

    registry.insert(skill("c-skill", "C"));
    let third = registry.catalog_delta_for_agent(None, 200_000);
    assert!(third.contains("c-skill"));
    assert!(!third.contains("a-skill"));
}
```

- [ ] **Step 2: Run and confirm failure**

Run:

```bash
cd src-tauri && cargo test --test skill_md_catalog_test -- --nocapture
```

Expected: FAIL because registry/catalog modules do not exist.

- [ ] **Step 3: Implement catalog formatter**

Create `src-tauri/src/plugin/skill/catalog_prompt.rs`:

```rust
use super::types::DiskSkill;

const SKILL_BUDGET_CONTEXT_PERCENT: f64 = 0.01;
const CHARS_PER_TOKEN: usize = 4;
const DEFAULT_CHAR_BUDGET: usize = 8_000;
const MAX_LISTING_DESC_CHARS: usize = 250;

pub fn format_skill_catalog_with_budget(skills: &[DiskSkill], context_window_tokens: usize) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut budget = if context_window_tokens == 0 {
        DEFAULT_CHAR_BUDGET
    } else {
        ((context_window_tokens as f64) * SKILL_BUDGET_CONTEXT_PERCENT) as usize * CHARS_PER_TOKEN
    };
    budget = budget.max(512);

    let mut lines = Vec::new();
    for skill in skills {
        let mut desc = skill.frontmatter.description.clone();
        if let Some(when) = &skill.frontmatter.when_to_use {
            desc.push_str(" ");
            desc.push_str(when);
        }
        if desc.chars().count() > MAX_LISTING_DESC_CHARS {
            desc = desc.chars().take(MAX_LISTING_DESC_CHARS).collect::<String>();
            desc.push('…');
        }
        lines.push(format!("- `{}` — {}", skill.id, desc));
    }

    let header = "The following skills are available for use with the load_skill tool:\n\n";
    let footer = "\nUse load_skill({ skill_id: \"<id>\" }) to load detailed instructions when a skill matches the user request.";
    let mut content = format!("{}{}{}", header, lines.join("\n"), footer);
    if content.len() > budget {
        content = format!(
            "{}{}{}",
            header,
            skills
                .iter()
                .map(|s| format!("- `{}`", s.id))
                .collect::<Vec<_>>()
                .join("\n"),
            footer
        );
    }
    content
}
```

- [ ] **Step 4: Implement registry**

Create `src-tauri/src/plugin/skill/registry.rs`:

```rust
use std::collections::{HashMap, HashSet};

use super::catalog_prompt::format_skill_catalog_with_budget;
use super::types::DiskSkill;

#[derive(Default)]
pub struct SkillRegistry {
    skills: HashMap<String, DiskSkill>,
    sent_skill_names: HashMap<String, HashSet<String>>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_skills(skills: Vec<DiskSkill>) -> Self {
        let mut registry = Self::new();
        for skill in skills {
            registry.insert(skill);
        }
        registry
    }

    pub fn insert(&mut self, skill: DiskSkill) {
        self.skills.insert(skill.id.clone(), skill);
    }

    pub fn get(&self, id: &str) -> Option<&DiskSkill> {
        self.skills.get(id)
    }

    pub fn skill_ids(&self) -> Vec<String> {
        let mut ids = self.skills.keys().cloned().collect::<Vec<_>>();
        ids.sort();
        ids
    }

    pub fn catalog_delta_for_agent(&mut self, agent_id: Option<&str>, context_window_tokens: usize) -> String {
        let key = agent_id.unwrap_or("").to_string();
        let sent = self.sent_skill_names.entry(key).or_default();
        let mut new_skills = self
            .skills
            .values()
            .filter(|skill| !sent.contains(&skill.id))
            .cloned()
            .collect::<Vec<_>>();
        new_skills.sort_by(|a, b| a.id.cmp(&b.id));
        if new_skills.is_empty() {
            return String::new();
        }
        for skill in &new_skills {
            sent.insert(skill.id.clone());
        }
        format_skill_catalog_with_budget(&new_skills, context_window_tokens)
    }

    pub fn reset_sent_skill_names(&mut self) {
        self.sent_skill_names.clear();
    }
}
```

- [ ] **Step 5: Export modules**

In `src-tauri/src/plugin/skill/mod.rs`, add:

```rust
pub mod catalog_prompt;
pub mod registry;
```

- [ ] **Step 6: Run tests**

Run:

```bash
cd src-tauri && cargo test --test skill_md_catalog_test -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/plugin/skill src-tauri/tests/skill_md_catalog_test.rs
git commit -m "feat(skill): add SKILL.md registry and catalog budget"
```

---

### Task 12: Implement variable substitution and shell execution

**Files:**
- Create: `src-tauri/src/plugin/skill/substitution.rs`
- Modify: `src-tauri/src/plugin/skill/mod.rs`
- Test: `src-tauri/tests/skill_md_substitution_test.rs`

- [ ] **Step 1: Write substitution tests**

Create `src-tauri/tests/skill_md_substitution_test.rs`:

```rust
use app_lib::plugin::skill::substitution::{substitute_skill_body, SkillSubstitutionContext};
use tempfile::TempDir;

#[test]
fn substitutes_skill_dir_session_and_arguments() {
    let dir = TempDir::new().unwrap();
    let ctx = SkillSubstitutionContext {
        skill_dir: dir.path().to_path_buf(),
        session_id: "session-123".to_string(),
        args: "北京 工程师".to_string(),
        argument_names: vec!["city".to_string(), "role".to_string()],
        execute_shell: false,
    };
    let body = "Dir=${AIJIA_SKILL_DIR}\nSession=${AIJIA_SESSION_ID}\nArgs=$ARGUMENTS\nCity=$city\nRole=$role\nFirst=$1";
    let result = substitute_skill_body(body, &ctx).unwrap();
    assert!(result.contains(&format!("Dir={}", dir.path().display())));
    assert!(result.contains("Session=session-123"));
    assert!(result.contains("Args=北京 工程师"));
    assert!(result.contains("City=北京"));
    assert!(result.contains("Role=工程师"));
    assert!(result.contains("First=北京"));
}

#[test]
fn appends_arguments_when_placeholder_absent() {
    let dir = TempDir::new().unwrap();
    let ctx = SkillSubstitutionContext {
        skill_dir: dir.path().to_path_buf(),
        session_id: "s".to_string(),
        args: "raw args".to_string(),
        argument_names: vec![],
        execute_shell: false,
    };
    let result = substitute_skill_body("body", &ctx).unwrap();
    assert!(result.contains("ARGUMENTS: raw args"));
}

#[test]
fn leaves_unknown_placeholders_unchanged() {
    let dir = TempDir::new().unwrap();
    let ctx = SkillSubstitutionContext {
        skill_dir: dir.path().to_path_buf(),
        session_id: "s".to_string(),
        args: "".to_string(),
        argument_names: vec![],
        execute_shell: false,
    };
    let result = substitute_skill_body("$unknown ${AIJIA_UNKNOWN}", &ctx).unwrap();
    assert!(result.contains("$unknown"));
    assert!(result.contains("${AIJIA_UNKNOWN}"));
}
```

- [ ] **Step 2: Run and confirm failure**

Run:

```bash
cd src-tauri && cargo test --test skill_md_substitution_test -- --nocapture
```

Expected: FAIL because substitution module does not exist.

- [ ] **Step 3: Implement substitution**

Create `src-tauri/src/plugin/skill/substitution.rs`:

```rust
use std::path::PathBuf;

use anyhow::{Context, Result};

pub struct SkillSubstitutionContext {
    pub skill_dir: PathBuf,
    pub session_id: String,
    pub args: String,
    pub argument_names: Vec<String>,
    pub execute_shell: bool,
}

pub fn substitute_skill_body(body: &str, ctx: &SkillSubstitutionContext) -> Result<String> {
    let parsed_args = shell_words::split(&ctx.args).unwrap_or_else(|_| {
        ctx.args
            .split_whitespace()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    });

    let mut out = body.to_string();
    out = out.replace("${AIJIA_SKILL_DIR}", &ctx.skill_dir.display().to_string());
    out = out.replace("${AIJIA_SESSION_ID}", &ctx.session_id);
    out = out.replace("$ARGUMENTS", &ctx.args);

    for idx in 0..9 {
        let value = parsed_args.get(idx).cloned().unwrap_or_default();
        out = out.replace(&format!("$ARGUMENTS[{idx}]"), &value);
        out = out.replace(&format!("${}", idx + 1), &value);
    }

    for (idx, name) in ctx.argument_names.iter().enumerate() {
        if let Some(value) = parsed_args.get(idx) {
            out = out.replace(&format!("${name}"), value);
        }
    }

    if !ctx.args.trim().is_empty() && !body.contains("$ARGUMENTS") {
        out.push_str("\n\nARGUMENTS: ");
        out.push_str(&ctx.args);
    }

    if ctx.execute_shell {
        out = execute_inline_shell_blocks(&out)?;
    }

    Ok(out)
}

fn execute_inline_shell_blocks(input: &str) -> Result<String> {
    let mut result = String::new();
    let mut rest = input;
    while let Some(start) = rest.find("!`") {
        result.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('`') else {
            result.push_str("!`");
            result.push_str(after);
            return Ok(result);
        };
        let cmd = &after[..end];
        let output = std::process::Command::new("bash")
            .arg("-lc")
            .arg(cmd)
            .output()
            .with_context(|| format!("failed to execute skill shell command: {cmd}"))?;
        if !output.status.success() {
            anyhow::bail!(
                "Skill body shell command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        result.push_str(&String::from_utf8_lossy(&output.stdout));
        rest = &after[end + 1..];
    }
    result.push_str(rest);
    Ok(result)
}
```

- [ ] **Step 4: Export substitution module**

In `src-tauri/src/plugin/skill/mod.rs`, add:

```rust
pub mod substitution;
```

- [ ] **Step 5: Run substitution tests**

Run:

```bash
cd src-tauri && cargo test --test skill_md_substitution_test -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/plugin/skill src-tauri/tests/skill_md_substitution_test.rs
git commit -m "feat(skill): substitute AIjia skill variables"
```

---

## Phase D: Wire load_skill, Catalog Injection, and Fork Mode

### Task 13: Rewrite load_skill for SKILL.md inline mode

**Files:**
- Modify: `src-tauri/src/runtime/tools/builtin/load_skill.rs`
- Modify: `src-tauri/src/plugin/registry.rs`
- Test: `src-tauri/tests/load_skill_skill_md_test.rs`

- [ ] **Step 1: Write inline load_skill tests**

Create `src-tauri/tests/load_skill_skill_md_test.rs`:

```rust
use std::fs;
use std::sync::{Arc, Mutex};

use app_lib::plugin::skill::loader::load_skill_roots;
use app_lib::plugin::skill::registry::SkillRegistry;
use app_lib::runtime::tools::builtin::load_skill::LoadSkillRuntimeTool;
use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
use serde_json::json;
use tempfile::TempDir;

fn write_skill(root: &std::path::Path, id: &str, body: &str) {
    let dir = root.join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {id}\ndescription: Test skill\narguments: name\n---\n\n{body}\n"),
    )
    .unwrap();
}

fn tool_ctx() -> ToolExecutionContext {
    ToolExecutionContext::new(
        app_lib::runtime::ids::SessionId::new("session-test"),
        app_lib::runtime::ids::RunId::new("run-test"),
        None,
        "tool-test".to_string(),
        app_lib::runtime::cancellation::CancellationToken::new(),
    )
}

#[tokio::test]
async fn load_skill_returns_substituted_skill_md_body() {
    let root = TempDir::new().unwrap();
    write_skill(root.path(), "hello-skill", "Hello $name from ${AIJIA_SKILL_DIR}");
    let skills = load_skill_roots(&[root.path().to_path_buf()]).unwrap();
    let registry = Arc::new(Mutex::new(SkillRegistry::from_skills(skills.into_values().collect())));
    let tool = LoadSkillRuntimeTool::new(registry);

    let result = tool
        .execute(json!({"skill_id": "hello-skill", "args": "Alice"}), tool_ctx())
        .await
        .expect("load_skill should succeed");

    assert!(result.content.contains("## hello-skill (hello-skill)"));
    assert!(result.content.contains("Hello Alice"));
    assert!(result.content.contains(root.path().to_str().unwrap()));
}

#[tokio::test]
async fn load_skill_rejects_missing_or_empty_skill_id() {
    let registry = Arc::new(Mutex::new(SkillRegistry::new()));
    let tool = LoadSkillRuntimeTool::new(registry);
    for input in [json!({}), json!({"skill_id": ""}), json!({"skill_id": "   "})] {
        let err = tool.execute(input, tool_ctx()).await.unwrap_err();
        assert!(err.to_string().contains("Missing required field: skill_id"));
    }
}
```

- [ ] **Step 2: Run and confirm failure**

Run:

```bash
cd src-tauri && cargo test --test load_skill_skill_md_test -- --nocapture
```

Expected: FAIL because `LoadSkillRuntimeTool` still uses old registry API.

- [ ] **Step 3: Rewrite LoadSkillRuntimeTool constructor**

In `src-tauri/src/runtime/tools/builtin/load_skill.rs`, change the struct to:

```rust
pub struct LoadSkillRuntimeTool {
    skill_registry: Arc<Mutex<SkillRegistry>>,
}
```

and constructor:

```rust
impl LoadSkillRuntimeTool {
    pub fn new(skill_registry: Arc<Mutex<SkillRegistry>>) -> Self {
        Self { skill_registry }
    }
}
```

Use `tokio::sync::Mutex` if the project already standardizes on async mutex; otherwise `std::sync::Mutex` is enough for short registry access.

- [ ] **Step 4: Update tool definition schema**

In `definition()`, include both `skill_id` and `args` in description text. If `ToolDefinition` has no JSON schema field, keep schema in description and catalog entry.

Description must include available IDs:

```rust
let ids = self.skill_registry.lock().unwrap().skill_ids().join(", ");
```

- [ ] **Step 5: Implement inline execute**

In `execute()`:

1. Read `skill_id` as trimmed string.
2. Read optional `args` as string default `""`.
3. Clone the `DiskSkill` out of registry.
4. Build `SkillSubstitutionContext` using `ctx.session_id`, skill root, args, and frontmatter arguments.
5. Call `substitute_skill_body` with `execute_shell: false` for the first pass. Shell execution permission integration is Task 15.
6. Return `ToolResult::new("load_skill", content, data)`.

Content format:

```text
## <frontmatter.name> (<skill_id>)

Base directory for this skill: <absolute path>

<body>
```

- [ ] **Step 6: Update request-scoped factory**

In `src-tauri/src/plugin/registry.rs`, update the `load_skill` factory branch to pass the new registry type.

If `RequestScopedRuntimeDeps.skill_registry` type changes in Task 14, temporarily keep adapter code here.

- [ ] **Step 7: Run tests**

Run:

```bash
cd src-tauri && cargo test --test load_skill_skill_md_test -- --nocapture
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/runtime/tools/builtin/load_skill.rs src-tauri/src/plugin/registry.rs src-tauri/tests/load_skill_skill_md_test.rs
git commit -m "feat(skill): load SKILL.md bodies via load_skill"
```

---

### Task 14: Wire disk SkillRegistry into app startup and user scope

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Modify: `src-tauri/src/plugin/registry.rs`
- Modify: `src-tauri/src/llm/sub_agent.rs`
- Modify: `src-tauri/src/llm/tool_executor/internal_system.rs`
- Test: `src-tauri/tests/runtime_dependencies_production_wiring_test.rs`

- [ ] **Step 1: Add SkillRegistry type to RequestScopedRuntimeDeps**

In `src-tauri/src/plugin/registry.rs`, set:

```rust
pub skill_registry: Option<Arc<Mutex<crate::plugin::skill::registry::SkillRegistry>>>,
```

Remove old `Arc<crate::plugin::SkillRegistry>` references.

- [ ] **Step 2: Load skills from both roots at startup/user scope activation**

In `src-tauri/src/lib.rs`, remove `scan_external_plugins` and replace with:

```rust
let global_skills_dir = aijia_home.skills_dir();
let user_skills_dir = current_user_storage
    .resolve_paths()
    .map(|paths| paths.skills_dir());
let roots = match user_skills_dir {
    Some(user) => vec![user, global_skills_dir],
    None => vec![global_skills_dir],
};
let skills = crate::plugin::skill::loader::load_skill_roots(&roots)?;
let skill_registry = Arc::new(Mutex::new(crate::plugin::skill::registry::SkillRegistry::from_skills(
    skills.into_values().collect(),
)));
app.manage(skill_registry.clone());
```

Adapt error handling to existing startup style.

- [ ] **Step 3: Inject registry into chat services**

In `src-tauri/src/transport/tauri_commands/chat.rs`, replace old `skill_registry` type in `TauriChatServices` with new registry type.

- [ ] **Step 4: Inject registry into sub-agent deps**

In `src-tauri/src/llm/sub_agent.rs`, ensure:

```rust
pub skill_registry: Option<Arc<Mutex<crate::plugin::skill::registry::SkillRegistry>>>,
```

and `request_scoped_tool_deps()` passes it through.

In `src-tauri/src/llm/tool_executor/internal_system.rs`, set:

```rust
skill_registry: ctx.skill_registry.clone(),
```

- [ ] **Step 5: Remove src-tauri/plugins scanning**

In `src-tauri/src/lib.rs`, delete all code walking:

```rust
src-tauri/plugins
```

or app resource plugin dirs. The only roots are global/user skills dirs.

- [ ] **Step 6: Add production wiring assertion**

In `src-tauri/tests/runtime_dependencies_production_wiring_test.rs`, add a test that constructs `SubAgentRuntimeDeps` with `skill_registry: Some(...)`, calls `request_scoped_tool_deps`, and asserts child deps keep `Some`.

- [ ] **Step 7: Verify no src-tauri/plugins scan remains**

Run:

```bash
grep -rn "src-tauri/plugins\|scan_external_plugins\|plugins_dir\|DeclarativeSkill" src-tauri/src src-tauri/tests --exclude-dir=target
```

Expected: no runtime scan references.

- [ ] **Step 8: Run wiring tests**

Run:

```bash
cd src-tauri && cargo test --test runtime_dependencies_production_wiring_test -- --nocapture
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/transport src-tauri/src/plugin/registry.rs src-tauri/src/llm src-tauri/tests/runtime_dependencies_production_wiring_test.rs
git commit -m "feat(skill): load skills from user and global SKILL.md roots"
```

---

### Task 15: Implement inline shell execution permissions and invoked skill tracking

**Files:**
- Modify: `src-tauri/src/plugin/skill/substitution.rs`
- Create: `src-tauri/src/plugin/skill/invoked.rs`
- Modify: `src-tauri/src/plugin/skill/mod.rs`
- Modify: `src-tauri/src/plugin/skill/registry.rs`
- Modify: `src-tauri/src/runtime/tools/builtin/load_skill.rs`
- Test: `src-tauri/tests/skill_md_substitution_test.rs`

- [ ] **Step 1: Add shell execution test**

In `src-tauri/tests/skill_md_substitution_test.rs`, add:

```rust
#[test]
fn executes_inline_shell_blocks_when_enabled() {
    let dir = TempDir::new().unwrap();
    let ctx = SkillSubstitutionContext {
        skill_dir: dir.path().to_path_buf(),
        session_id: "s".to_string(),
        args: "".to_string(),
        argument_names: vec![],
        execute_shell: true,
    };
    let result = substitute_skill_body("before !`printf hello` after", &ctx).unwrap();
    assert_eq!(result, "before hello after");
}
```

- [ ] **Step 2: Add invoked skill data structure**

Create `src-tauri/src/plugin/skill/invoked.rs`:

```rust
use std::collections::HashMap;
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct InvokedSkillInfo {
    pub skill_id: String,
    pub body: String,
    pub invoked_at: SystemTime,
}

#[derive(Default)]
pub struct InvokedSkillStore {
    entries: HashMap<String, InvokedSkillInfo>,
}

impl InvokedSkillStore {
    pub fn remember(&mut self, agent_id: Option<&str>, skill_id: &str, body: String) {
        let key = format!("{}:{}", agent_id.unwrap_or(""), skill_id);
        self.entries.insert(
            key,
            InvokedSkillInfo {
                skill_id: skill_id.to_string(),
                body,
                invoked_at: SystemTime::now(),
            },
        );
    }
}
```

- [ ] **Step 3: Export invoked module**

In `src-tauri/src/plugin/skill/mod.rs`, add:

```rust
pub mod invoked;
```

- [ ] **Step 4: Track invoked skills after inline load**

In `SkillRegistry`, add an `invoked: InvokedSkillStore` field and a method:

```rust
pub fn remember_invoked(&mut self, agent_id: Option<&str>, skill_id: &str, body: String) {
    self.invoked.remember(agent_id, skill_id, body);
}
```

Call it from `LoadSkillRuntimeTool::execute()` after successful inline substitution.

- [ ] **Step 5: Run substitution tests**

Run:

```bash
cd src-tauri && cargo test --test skill_md_substitution_test -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/plugin/skill src-tauri/src/runtime/tools/builtin/load_skill.rs src-tauri/tests/skill_md_substitution_test.rs
git commit -m "feat(skill): execute skill shell blocks and track invoked skills"
```

---

### Task 16: Implement fork mode load_skill

**Files:**
- Modify: `src-tauri/src/runtime/tools/builtin/load_skill.rs`
- Modify: `src-tauri/src/llm/sub_agent.rs`
- Modify: `src-tauri/src/runtime/agent/worker_runtime.rs`
- Test: `src-tauri/tests/load_skill_skill_md_test.rs`

- [ ] **Step 1: Add fork mode test with fake executor hook**

In `src-tauri/tests/load_skill_skill_md_test.rs`, add a test skill with:

```yaml
context: fork
agent: general-purpose
```

Test should assert `load_skill` returns content containing:

```text
Skill "fork-skill" completed (forked execution).
```

If no fake subagent executor is injectable yet, write a lower-level unit test for a helper function:

```rust
format_fork_result("fork-skill", "child output")
```

- [ ] **Step 2: Add helper formatter**

In `load_skill.rs`, add:

```rust
pub fn format_fork_result(skill_name: &str, result_text: &str) -> String {
    format!("Skill \"{}\" completed (forked execution).\n\nResult:\n{}", skill_name, result_text)
}
```

- [ ] **Step 3: Implement fork branch**

In `LoadSkillRuntimeTool::execute()`, if `skill.frontmatter.context.as_deref() == Some("fork")`:

1. Build child prompt from substituted body.
2. Use available `agent_runtime` / `SubAgentRuntimeDeps` from `ToolExecutionContext` or request-scoped deps.
3. Run child agent with allowed tools inherited from skill frontmatter.
4. Return `format_fork_result` content.

If current `ToolExecutionContext` lacks enough deps, extend it deliberately; do not use globals.

- [ ] **Step 4: Ensure subagent has skill_registry**

Verify `SubAgentRuntimeDeps::request_scoped_tool_deps()` sets:

```rust
skill_registry: self.skill_registry.clone(),
skill_sessions: None,
```

- [ ] **Step 5: Run fork tests**

Run:

```bash
cd src-tauri && cargo test --test load_skill_skill_md_test fork -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/tools/builtin/load_skill.rs src-tauri/src/llm/sub_agent.rs src-tauri/src/runtime/agent/worker_runtime.rs src-tauri/tests/load_skill_skill_md_test.rs
git commit -m "feat(skill): support forked SKILL.md execution"
```

---

### Task 17: Inject skill catalog through dynamic context

**Files:**
- Modify: `src-tauri/src/runtime/chat/context_builder.rs`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Test: `src-tauri/tests/s4_driver_loop_test.rs`

- [ ] **Step 1: Ensure context builder accepts only skill_catalog**

`build_iteration_context` should include:

```rust
skill_catalog: &str
```

and append it only when non-empty:

```rust
if !skill_catalog.is_empty() {
    ctx.push_str("\n\n<system-reminder>\n");
    ctx.push_str(skill_catalog);
    ctx.push_str("\n</system-reminder>");
}
```

- [ ] **Step 2: Add driver test for catalog injection**

In `src-tauri/tests/s4_driver_loop_test.rs`, ensure there is a test asserting dynamic context contains:

```text
The following skills are available for use with the load_skill tool
```

and does not contain:

```text
switch_skill
precompute_result
```

- [ ] **Step 3: Fetch catalog delta once per turn**

In `chat_turn_driver.rs`, before the iteration loop, call executor method:

```rust
let skill_catalog = executor.get_skill_catalog(request.agent_id.as_deref()).await;
```

If no agent id string exists, pass `None`.

- [ ] **Step 4: Implement get_skill_catalog in Tauri executor**

In `transport/tauri_commands/chat.rs`, lock `SkillRegistry` and call:

```rust
registry.catalog_delta_for_agent(agent_id, context_window_tokens)
```

Use existing model context window if available; otherwise pass `200_000`.

- [ ] **Step 5: Run driver tests**

Run:

```bash
cd src-tauri && cargo test --test s4_driver_loop_test driver_injects_skill_catalog_into_dynamic_context -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/chat src-tauri/src/transport/tauri_commands/chat.rs src-tauri/tests/s4_driver_loop_test.rs
git commit -m "feat(skill): inject SKILL.md catalog into dynamic context"
```

---

### Task 17b: Reshape `list_skills` IPC to expose only SKILL.md skills

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/skills.rs` 或 `src-tauri/src/commands/skill_management.rs` 中实现 `list_skills` 的位置（用 grep 定位）
- Modify: `src-tauri/src/lib.rs` 中 `app.manage(...)` 注册 `SkillRegistry` 的位置
- Test: 新增 `src-tauri/tests/list_skills_returns_skill_md_only_test.rs`

**Context:** 旧 `list_skills` 命令返回所有 `SkillRegistry` 中注册过的 skill，包括 builtin `daily-assistant` 与所有 `DeclarativeSkill`。新架构下注册表只装 SKILL.md skill，但需要：

1. 命令仍然存在（前端 `useSkillStore.reload()` / 管理页 / 导入页都依赖它）。
2. 返回的 `SkillInfo` 字段对前端兼容（`id` / `displayName` / `description` / `icon` / `category`）。
3. 不返回 `daily-assistant`（前端 WelcomeScreen 已经在过滤它，但后端不再注册它本就不会出现）。
4. 同 id 用户级 skill 覆盖公共级 skill（与 catalog 行为一致）。

- [ ] **Step 1: 定位现有 list_skills 实现**

```bash
grep -rn "fn list_skills\|\"list_skills\"" src-tauri/src --exclude-dir=target
```

记录命令所在文件路径。

- [ ] **Step 2: 写失败测试**

新建 `src-tauri/tests/list_skills_returns_skill_md_only_test.rs`：

```rust
use std::fs;
use std::sync::{Arc, Mutex};

use app_lib::plugin::skill::loader::load_skill_roots;
use app_lib::plugin::skill::registry::SkillRegistry;
use tempfile::TempDir;

fn write_skill(root: &std::path::Path, id: &str, label: &str) {
    let dir = root.join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {id}\ndescription: desc {id}\nmetadata:\n  label: {label}\n---\nbody"),
    )
    .unwrap();
}

#[test]
fn list_skills_returns_only_skill_md_entries() {
    let global = TempDir::new().unwrap();
    let user = TempDir::new().unwrap();
    write_skill(global.path(), "biz-writing", "商务写作");
    write_skill(user.path(), "salary-query", "薪酬查询");
    write_skill(user.path(), "biz-writing", "用户覆盖");

    let skills = load_skill_roots(&[user.path().to_path_buf(), global.path().to_path_buf()]).unwrap();
    let registry = Arc::new(Mutex::new(SkillRegistry::from_skills(skills.into_values().collect())));

    // 调 Tauri 命令的纯函数（实现见 Step 4）。
    let infos = app_lib::commands::skill_management::list_skills_from_registry(&registry);

    let ids = infos.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
    assert!(ids.contains(&"salary-query".to_string()));
    assert!(ids.contains(&"biz-writing".to_string()));
    let biz = infos.iter().find(|s| s.id == "biz-writing").unwrap();
    assert_eq!(biz.display_name, "用户覆盖", "user scope must override global");
    assert!(!ids.contains(&"daily-assistant".to_string()));
}
```

- [ ] **Step 3: 跑测试确认失败**

```bash
cd src-tauri && cargo test --test list_skills_returns_skill_md_only_test -- --nocapture
```

Expected: FAIL — `list_skills_from_registry` 不存在。

- [ ] **Step 4: 实现纯函数 + 命令包装**

在 Step 1 定位到的命令文件中（假设 `src-tauri/src/commands/skill_management.rs`），增加：

```rust
use std::sync::{Arc, Mutex};

use crate::plugin::skill::registry::SkillRegistry;
use crate::plugin::skill::types::DiskSkill;

/// 前端 `useSkillStore` 期望的字段。保留旧字段名兼容。
#[derive(Debug, Clone, serde::Serialize)]
pub struct SkillInfo {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub icon: Option<String>,
    pub category: Option<String>,
}

pub fn list_skills_from_registry(registry: &Arc<Mutex<SkillRegistry>>) -> Vec<SkillInfo> {
    let guard = registry.lock().unwrap();
    let mut entries: Vec<&DiskSkill> = guard.skill_ids().iter().filter_map(|id| guard.get(id)).collect();
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    entries
        .into_iter()
        .map(|skill| SkillInfo {
            id: skill.id.clone(),
            display_name: skill
                .frontmatter
                .metadata
                .label
                .clone()
                .unwrap_or_else(|| skill.frontmatter.name.clone()),
            description: skill.frontmatter.description.clone(),
            icon: None,
            category: None,
        })
        .collect()
}

#[tauri::command]
pub fn list_skills(state: tauri::State<'_, Arc<Mutex<SkillRegistry>>>) -> Vec<SkillInfo> {
    list_skills_from_registry(state.inner())
}
```

> 如果项目已有不同的 `SkillInfo` 类型（可能定义在 `transport` 层），改成复用现有类型，并保留 `id` / `display_name` / `description` 三字段一致；前端 `useSkillStore` 不需要 `icon` / `category` 来工作，缺失时填 `None`。

- [ ] **Step 5: 注册命令 + state**

在 `src-tauri/src/lib.rs` 的 Tauri builder 中：

1. 确保 `app.manage(skill_registry.clone())` 已经在 Task 14 注册（会被 `state: tauri::State<'_, Arc<Mutex<SkillRegistry>>>` 解析）。
2. 在 `.invoke_handler(tauri::generate_handler![...])` 中保留 `list_skills`，删除任何指向旧 builtin 注册函数的版本。

- [ ] **Step 6: 跑测试确认通过**

```bash
cd src-tauri && cargo test --test list_skills_returns_skill_md_only_test -- --nocapture
```

Expected: PASS。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/commands/skill_management.rs src-tauri/src/lib.rs src-tauri/tests/list_skills_returns_skill_md_only_test.rs
git commit -m "feat(skill): list_skills returns SKILL.md skills only"
```

---

## Phase E: Frontend State Cleanup (Minimal)

> 决策：UI 入口（WelcomeScreen 卡片、底部 popover、SlashCommandPopover）**保留**。Phase E 只做"切断前端选中态对后端的影响"和"删除已经无效的死代码"两件事。原因：Task 17b 之后 `list_skills` 只返回新 SKILL.md skill，UI 自然只展示新 skill，不需要删 UI 组件。

### Task 18: Stop sending `selectedSkillId` to backend and stop parsing slash commands as skill selection

**Files:**
- Modify: `src/lib/tauri.ts`
- Modify: `src/hooks/useChat.ts`
- Delete: `src/hooks/useChat.skill.test.ts`
- Test: 新增/调整 `src/hooks/useChat.test.ts`

- [ ] **Step 1: 删除 legacy 前端测试**

```bash
git rm src/hooks/useChat.skill.test.ts
```

- [ ] **Step 2: 移除 IPC 参数**

在 `src/lib/tauri.ts` 中找到 `sendMessage` 定义（约 `line 253`），删除 `selectedSkillId` / `selectedSkillLabel` 参数与对应 invoke payload 字段：

```ts
export async function sendMessage(
  conversationId: string,
  clientMessageId: string,
  content: string,
  fileIds: string[],
): Promise<void> {
  return invoke('send_message', {
    conversationId,
    clientMessageId,
    content,
    fileIds,
  })
}
```

- [ ] **Step 3: 删除 slash 解析**

在 `src/hooks/useChat.ts` 中找到 `resolveManualSkillCommand` 函数（约 `line 42-63`）和它的调用点（`line 295` 附近的 `effectiveSelectedSkillId`），整段删除。`/salary-query xxx` 类输入不再特殊处理，作为普通 message content 直接发送。

- [ ] **Step 4: 添加新测试**

在 `src/hooks/useChat.test.ts` 中加：

```ts
it('sends slash-prefixed text verbatim without selectedSkillId', async () => {
  const sendMessageMock = vi.mocked(sendMessage)
  // ...usual setup...
  await result.current.sendUserMessage('/salary-query 北京 算法工程师')
  expect(sendMessageMock).toHaveBeenCalledWith(
    expect.any(String),
    expect.any(String),
    '/salary-query 北京 算法工程师',
    [],
  )
})
```

- [ ] **Step 5: 跑前端测试**

```bash
pnpm exec vitest run src/hooks/useChat.test.ts src/lib/tauri.skills.test.ts
```

Expected: PASS。

- [ ] **Step 6: 提交**

```bash
git add src/lib/tauri.ts src/hooks/useChat.ts src/hooks/useChat.test.ts
git rm src/hooks/useChat.skill.test.ts
git commit -m "refactor(skill): stop forwarding selectedSkillId from chat send"
```

---

### Task 19: Remove `selectedSkillCommands` state and its setters

**Files:**
- Modify: `src/stores/chatStore.ts`
- Modify: `src/hooks/useSkillComposer.ts`
- Modify: `src/components/chat-scene/ChatBottomArea.tsx`
- Modify: `src/stores/authStore.ts`
- Test: `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`

**Context:** 即便后端忽略 `selectedSkillId`，前端 `useChatStore.selectedSkillCommands` 这个 state 仍然在 popover 点击时被 set，UI 会显示"已选 skill"状态。本任务把这个 state 和它的 reducer/调用者统一删除，让 popover 点击不再"挂选中态"。popover 与卡片 UI 本身保留，作为"列表浏览"和"插入文本"的载体。

- [ ] **Step 1: 删除 chatStore 中的 selectedSkillCommands**

在 `src/stores/chatStore.ts` 中删除：
- `line 35-37` 的字段与 reducer 类型
- `line 122-136` 的初始值与 `setSelectedSkillCommand` / `clearSelectedSkillCommand` 实现
- 顶部 `ComposerSkillCommand` 的 import / 类型定义（如果只有这两处用）

- [ ] **Step 2: 删除 authStore 中的清理调用**

在 `src/stores/authStore.ts` 中删除两处：

```ts
useChatStore.setState({ selectedSkillCommands: {} })
```

（约 `line 89` 与 `line 106`）。

- [ ] **Step 3: 删除 useSkillComposer 中的写入动作**

在 `src/hooks/useSkillComposer.ts` 中（约 `line 53`），删除 `setSelectedSkillCommand` 调用：

```ts
useChatStore.getState().setSelectedSkillCommand(conversationId, { ... })
```

popover 点击的副作用改为：把 skill 的 `triggerText` 插入到当前 composer input（已有的输入文本前面或替换占位符），不再设置任何选中态。如果 hook 已经返回 `triggerText`，确认 ChatBottomArea 调用方读到后正确把它写入 input ref。

- [ ] **Step 4: 删除 ChatBottomArea 中对 selectedSkillCommand 的所有引用**

在 `src/components/chat-scene/ChatBottomArea.tsx` 中删除：

- `line 89-90` 的 `selectedSkillCommand` / `clearSelectedSkillCommand` 取值
- `line 144-145` payload 中的 `selectedSkillId` / `selectedSkillCommand` 字段
- `line 175` 与 `line 275` 的 `clearSelectedSkillCommand` 调用
- `line 182` 的 useCallback 依赖列表中的相关项

popover JSX 保留；只移除"挂选中态"的副作用。

- [ ] **Step 5: 删除 useChat.ts 中读 selectedSkillCommands 的代码**

在 `src/hooks/useChat.ts:308` 与 `line 485-486` 删除读取/写入 `selectedSkillCommands` 的代码（这些应该已经在 Task 18 删 `effectiveSelectedSkillId` 的同一片段内）。

- [ ] **Step 6: 更新 ChatBottomArea 测试**

在 `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx` 中：

1. 删除所有 `selectedSkillCommands: { ... }` ��测试 setup 与断言（约 `line 73, 116, 142, 152, 170, 195`）。
2. 添加新测试：mock `useSkillComposer` 返回某 skill 的 `triggerText`，模拟点击 popover 项，断言 `composer.input` 文本被设置为 `triggerText`，且 `useChatStore.getState()` 上没有 `selectedSkillCommands` 字段。

- [ ] **Step 7: 跑前端测试**

```bash
pnpm exec vitest run src/components/chat-scene/__tests__/ChatBottomArea.test.tsx src/stores/chatStore.test.ts src/stores/authStore.test.ts
```

Expected: PASS。

- [ ] **Step 8: 提交**

```bash
git add src/stores src/hooks/useSkillComposer.ts src/hooks/useChat.ts src/components/chat-scene
git commit -m "refactor(skill): drop selectedSkillCommands client state"
```

---

### Task 20: Remove dead `HIDDEN_TOOLS = ['switch_skill']` branch

**Files:**
- Modify: `src/hooks/useStreaming.ts`
- Test: `src/hooks/useStreaming.integration.test.tsx`

- [ ] **Step 1: 删除常量与分支**

在 `src/hooks/useStreaming.ts:356-357` 与 `line 384-392` 删除：

```ts
const HIDDEN_TOOLS = ['switch_skill']
```

以及所有 `if (HIDDEN_TOOLS.includes(...))` 分支。`load_skill` 走通用工具气泡。

- [ ] **Step 2: 调整测试**

在 `src/hooks/useStreaming.integration.test.tsx` 中删除任何"switch_skill 应被隐藏"的测试。如果有的话，新增一条测试断言 `load_skill` 工具事件正常进入 streaming bubbles。

- [ ] **Step 3: 跑测试**

```bash
pnpm exec vitest run src/hooks/useStreaming.integration.test.tsx
```

Expected: PASS。

- [ ] **Step 4: 提交**

```bash
git add src/hooks/useStreaming.ts src/hooks/useStreaming.integration.test.tsx
git commit -m "chore(skill): remove dead switch_skill streaming filter"
```

---

## Phase F: Final Verification and Documentation

### Task 22: Add review tests forbidding legacy paths

**Files:**
- Create: `src-tauri/tests/review_skill_system_no_legacy_test.rs`

- [ ] **Step 1: Create review test**

Create `src-tauri/tests/review_skill_system_no_legacy_test.rs`:

```rust
use std::fs;
use std::path::Path;

fn repo_file(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(path))
        .unwrap_or_else(|err| panic!("failed to read {path}: {err}"))
}

#[test]
fn production_code_no_longer_references_legacy_skill_files() {
    let forbidden = [
        "plugin.toml",
        "workflow.toml",
        "switch_skill",
        "SkillSessionStore",
        "SkillRuntimePatch",
        "selected_skill_id",
        "selectedSkillId",
        "precompute_result",
        "is_analysis",
    ];
    let roots = ["src-tauri/src", "src"];
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for root in roots {
        for entry in walkdir::WalkDir::new(repo.join(root)) {
            let entry = entry.unwrap();
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()).is_none() {
                continue;
            }
            let content = fs::read_to_string(path).unwrap_or_default();
            for needle in forbidden {
                assert!(
                    !content.contains(needle),
                    "legacy skill marker `{needle}` found in {}",
                    path.display()
                );
            }
        }
    }
}
```

If `walkdir` is not already a dependency, add `walkdir = "2"` to `dev-dependencies` or implement recursive traversal manually.

- [ ] **Step 2: Run review test**

Run:

```bash
cd src-tauri && cargo test --test review_skill_system_no_legacy_test -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tests/review_skill_system_no_legacy_test.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "test(skill): forbid legacy stateful skill markers"
```

---

### Task 23: Update docs and stale plans index

**Files:**
- Modify: `CLAUDE.md`
- Modify: `docs/test-intents/context/context.md`
- Modify: `docs/test-intents/spec/tasks/skill-loading/rules.md`
- Modify: `docs/test-intents/spec/tasks/skill-loading/test-progress.md`
- Modify: `docs/superpowers/plans/README.md`
- Modify: `docs/skill-system-comparison.md`

- [ ] **Step 1: Update CLAUDE.md skill section**

Replace references to `plugin.toml`, `workflow.toml`, `switch_skill`, `SkillSessionStore` with:

```markdown
Skill 系统（新）：只加载 `~/.renlijia/users/{scope}/skills/*/SKILL.md` 与 `~/.renlijia/skills/*/SKILL.md`。LLM 通过 `load_skill` 无状态加载 SKILL.md body；不再存在 switch_skill / SkillSessionStore / workflow pipeline。
```

- [ ] **Step 2: Update test-intents context**

In `docs/test-intents/context/context.md`, change skill id source from `plugin.toml` to `SKILL.md` frontmatter / directory id.

- [ ] **Step 3: Rewrite skill-loading rules**

In `docs/test-intents/spec/tasks/skill-loading/rules.md`, replace old rules with:

```markdown
# Skill loading rules

- Rule 1: Runtime loads only SKILL.md directories from user and global skill roots.
- Rule 2: User-scope skill overrides global skill with the same id.
- Rule 3: plugin.toml/workflow.toml-only directories are ignored.
- Rule 4: load_skill returns expanded SKILL.md body and never persists active skill state.
- Rule 5: selected_skill_id is not accepted by chat send_message.
```

- [ ] **Step 4: Mark stale plans superseded**

In `docs/superpowers/plans/README.md`, mark old skill plans as superseded by this plan:

- `2026-04-21-skill-system-complete-overhaul.md`
- `2026-04-25-skill-command-claude-code-best-alignment.md`
- `2026-04-25-skill-command-end-to-end.md`
- `2026-04-26-llm-skill-routing.md`
- `2026-04-27-skill-system-stateless-migration.md`

- [ ] **Step 5: Update comparison doc**

In `docs/skill-system-comparison.md`, replace the old comparison with a short note:

```markdown
# Skill System Comparison

This document is superseded by `docs/superpowers/specs/2026-04-28-aijia-skill-spec.md` and `docs/superpowers/plans/2026-04-28-aijia-skill-system-rewrite.md`.

AIjia no longer supports the legacy `plugin.toml + workflow.toml` skill format.
```

- [ ] **Step 6: Commit**

```bash
git add CLAUDE.md docs/test-intents docs/superpowers/plans/README.md docs/skill-system-comparison.md
git commit -m "docs(skill): document SKILL.md-only architecture"
```

---

### Task 24: Full verification

**Files:**
- No source changes expected

- [ ] **Step 1: Run focused Rust tests serially**

```bash
cd src-tauri && cargo test --test skill_md_frontmatter_test -- --nocapture
cd src-tauri && cargo test --test skill_md_loader_test -- --nocapture
cd src-tauri && cargo test --test skill_md_substitution_test -- --nocapture
cd src-tauri && cargo test --test skill_md_catalog_test -- --nocapture
cd src-tauri && cargo test --test load_skill_skill_md_test -- --nocapture
cd src-tauri && cargo test --test review_skill_system_no_legacy_test -- --nocapture
```

Expected: all PASS.

- [ ] **Step 2: Run existing key Rust regression tests serially**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast
cd src-tauri && cargo test --test builtin_runtime_registration_test -- --nocapture
cd src-tauri && cargo test --test runtime_dependencies_production_wiring_test -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Run frontend tests**

```bash
pnpm exec vitest run src/lib/tauri.skills.test.ts src/hooks/useChat.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts
```

Expected: PASS.

- [ ] **Step 4: Run lint/build checks**

```bash
pnpm lint
pnpm build
cd src-tauri && cargo check --quiet
```

Expected: PASS.

- [ ] **Step 5: Manual app verification**

Run:

```bash
pnpm tauri:dev
```

Manual checks:

1. Put a test skill under `~/.renlijia/skills/salary-query/SKILL.md`.
2. New chat: ask "北京算法工程师薪资怎么样".
3. Verify model sees catalog and calls `load_skill`.
4. Verify no `switch_skill` tool event appears.
5. Type `/salary-query 北京 算法工程师` and verify it is sent as normal text, not parsed into selected skill metadata.

- [ ] **Step 6: Commit verification fixes if needed**

If any verification fixes were made:

```bash
git add <changed files>
git commit -m "fix(skill): resolve SKILL.md rewrite verification issues"
```

---

## Self-Review Notes

Spec coverage:

- Disk format and frontmatter: Tasks 9–10.
- User/global roots and precedence: Tasks 10, 14.
- Variable substitution and shell blocks: Tasks 12, 15.
- inline load_skill: Task 13.
- fork mode: Task 16.
- catalog 1% budget and incremental sent names: Task 11, Task 17.
- legacy deletion: Tasks 2–8, 18–21, 22.
- docs/test-intents update: Task 23.
- final verification: Task 24.

Known implementation caveats:

- Some exact line numbers will drift because earlier tasks delete files. Use symbol names and grep commands in each task.
- The plan intentionally accepts broken intermediate commits during Phase A/B because the user requested one-time full alignment rather than compatibility.
- If `walkdir` is not available, Task 22 allows manual recursive traversal instead of adding a dev dependency.
