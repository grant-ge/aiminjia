# RepoWiki Log

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
