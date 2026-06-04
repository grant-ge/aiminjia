# RepoWiki Log

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
