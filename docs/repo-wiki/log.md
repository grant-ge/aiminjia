# RepoWiki Log

## 2026-06-15

- Confirmed `origin/main` / local `main` at `1744d119` contains the RepoWiki merge (`e043aa57`) and the original Understand-Anything RepoWiki branch (`f60bbfe9`).
- Replaced the earlier Agent foreground auto-background gap writeback with current-source implementation coverage in `runtime-agent-foreground-auto-background.json`: default `SpawnSubagent` foreground path now promotes long-running Agent work to `task_type=local_agent` after the blocking budget, while explicit `run_in_background=true` still uses direct async launch.
- Updated `runtime-shell-auto-background.json` and `runtime-map.md` so Shell `LocalBash` handoff and Agent `LocalAgent` promotion are both covered without treating Agent auto-background as an unresolved gap.
- Added `runtime-shell-auto-background.json` for Bash/PowerShell foreground command auto-backgrounding, LocalBash background task registration, transcript handoff, TaskOutput, TaskStop and task-notification visibility.
- Earlier in the day, recorded the target Agent/Subagent foreground auto-background behavior as a gap; this has now been superseded by current-source implementation coverage in `runtime-agent-foreground-auto-background.json`.
- Clarified the current boundary that AIjia now has both explicit `run_in_background=true` async subagent launch and default foreground auto-promotion; Claude code best remains a design reference, not a current-source evidence path.
- Cross-validated the writeback with Dirac (`gpt-5.3-codex-spark`), Lagrange (`gpt-5.4`) and Russell (`gpt-5.4-mini`): current source boundary, Claude code best full-chain reference and existing graph gap were checked separately.
- Updated `runtime-map.md`, `coverage-manifest.md`, `writeback-queue.md` and `index.md` to expose both implemented Shell `LocalBash` auto-background and Agent `LocalAgent` foreground auto-background domains.
- Graph snapshot after merge: 9357 nodes, 10345 edges, 25 layers, 116 guided tour steps, 411 LLM-enhanced nodes, 128 architecture review nodes and 25 enhancement files.
- Checked the latest `origin/main` after the user asked whether the wiki is complete. Commit `b0152fee` adds AIjia gateway v2 visible reply language anchoring in `src-tauri/src/llm/providers/aijia_gateway_v2.rs`; commit `c4bcc8b7` adds Chinese visible reply regression intents in `docs/test-intents/spec/tasks/对话/rules.md`.
- Added `.understand-anything/enhancements/llm-visible-reply-language-anchor.json` from target-branch source `origin/main@c4bcc8b7e4c12e622e91def848278e051b754c72`, and updated `runtime-map.md`, `coverage-manifest.md`, `writeback-queue.md` and `index.md`.
- Fast-forwarded local `main` to `origin/main@c4bcc8b7` and rebased the wiki update branch on top of the updated main before pushing, so the wiki commit is no longer based on the stale `1744d119` main.
- Graph snapshot after visible language merge: 9361 nodes, 10353 edges, 25 layers, 119 guided tour steps, 415 LLM-enhanced nodes, 132 architecture review nodes and 26 enhancement files.

## 2026-06-04

- Updated the repo-local `userwiki` skill with a Karpathy-style LLM Wiki mental model.
- Added `coverage-manifest.md` and `writeback-queue.md` so UserWiki coverage and writeback gaps are tracked explicitly before continuing module supplementation.
- Added `references/llm-wiki-principles.md` to both `.agents/skills/userwiki/` and `.claude/skills/userwiki/`.
- Clarified that UserWiki should be treated as an LLM-maintained knowledge intermediate layer: raw source -> compiled wiki/enhancement -> query -> writeback -> lint/QA.
- Updated `scripts/check-repowiki.mjs` so the new userwiki reference is validated in both skill mirrors.
- Merged 7 new enhancement files into `.understand-anything/knowledge-graph.json`: employee dispatch, agenda scheduler, task tools, team mode/subagent, IM channel core, skill registry/sync and UserWiki LLM Wiki principles.
- Added a UserWiki QA smoke case for the LLM Wiki method question.
- Graph snapshot after merge: 9299 nodes, 9975 edges, 25 layers, 77 guided tour steps, 274 LLM-enhanced nodes, 73 architecture review nodes and 16 enhancement files.
- Added `app-shell-settings-updater-billing.json` for App shell, settings, updater, billing and network boundaries.
- Final graph snapshot after app shell merge: 9307 nodes, 10056 edges, 25 layers, 83 guided tour steps, 299 LLM-enhanced nodes, 81 architecture review nodes and 17 enhancement files.
- Added `prompt-context-compaction-cost.json` for prompt assembly, dynamic context, compaction, usage and cost accounting. Graph snapshot after merge: 9314 nodes, 10099 edges, 25 layers, 87 guided tour steps, 314 LLM-enhanced nodes, 88 architecture review nodes and 18 enhancement files.
- Added `test-intents-aijia-cli.json` for AEIT/test-intents, `aijia` CLI entrypoints, repo-local intent skills and rules/report navigation. Graph snapshot after merge: 9320 nodes, 10121 edges, 25 layers, 90 guided tour steps, 329 LLM-enhanced nodes, 93 architecture review nodes and 19 enhancement files.
- Added `billing-subscription-account-network.json`, `tauri-command-event-contracts.json` and `user-scope-auth-storage-boundary.json` for billing/account/network, cross-layer IPC/event contracts and user-scope auth/storage boundaries. Graph snapshot after merge: 9337 nodes, 10232 edges, 25 layers, 100 guided tour steps, 367 LLM-enhanced nodes, 110 architecture review nodes and 22 enhancement files.
- Added `context-budget-truncation-matrix.json` for long-dialogue forgetting, hardcoded context budgets, effectiveness labels and local compact-boundary troubleshooting. Updated UserWiki troubleshooting mirrors and corrected QueryEngine budget wording from unconfirmed to ordinary chat main chain not wired. Graph snapshot after merge: 9343 nodes, 10274 edges, 25 layers, 105 guided tour steps, 382 LLM-enhanced nodes, 116 architecture review nodes and 23 enhancement files.
- Added a tag/commit intake rule to `wiki-maintainer` and queued two main-delta writebacks from `v0.5.33..main` / `v0.5.33..origin/main`: app data root contract and runtime cache reinstall / bundled fallback. These remain candidate items until the target main source tree is used for current-source enhancement and validation.

## 2026-06-03

- Initialized `docs/repo-wiki/` from `.understand-anything/knowledge-graph.json`.
- Added graph-derived architecture, runtime, frontend, testing, source and decision index pages.
- Added repo-local `userwiki` and `wiki-maintainer` skills in `.agents/skills/` and `.claude/skills/`.
- Added `scripts/check-repowiki.mjs` for deterministic validation.

Graph snapshot:

- Nodes: 9249
- Edges: 9681
- Layers: 25
- Guided tour steps: 40
- LLM-enhanced nodes: 139
- Architecture review nodes: 27
- Current-source enhancement files: 9
- Language: zh

Code/test graph enhancement:

- Added `.understand-anything/enhancements/*.json` as structured sub-agent outputs.
- Merged 8 code/test enhancement files with `scripts/apply-understand-enhancements.mjs`.
- Updated 106 code/test-backed key nodes.
- Added 169 semantic edges.
- Added 24 architecture review concept nodes under `layer:architecture-review`.
- Added 23 guided tour steps.
- Validation: `node scripts/check-repowiki.mjs`; Understand-Anything schema validation reported success with 0 issues.

UserWiki skill enhancement:

- Renamed the maintenance skill from `repo-wiki-maintainer` to `wiki-maintainer`.
- Added `userwiki` as the user-facing install, usage and Q&A entrypoint.
- Added UserWiki Q&A playbook for questions such as "I want to add a feature; what will it affect?"
- Added real UserWiki QA examples and CLI smoke fixtures for testing answer quality.
- Added `.understand-anything/enhancements/userwiki-skill-system.json` and merged it into the graph.
- Added 14 current-source skill/script nodes, 14 semantic edges and 3 guided tour steps for the UserWiki skill system.
- Updated `scripts/check-repowiki.mjs` to validate both skills and their `.agents` / `.claude` mirrors.
