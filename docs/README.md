# Aijia Documentation

This directory keeps active desktop documentation, operational playbooks, and maintained
test assets. Historical execution plans, handoffs, reviews, and superseded specs are
archived under `docs/archive/`.

## Current References

- `architecture-blueprint.md`: current architecture overview.
- `repo-wiki/`: Understand-Anything graph-derived repository wiki and onboarding map.
- `runtime-manager.md`: runtime manager reference.
- `prompt-architecture.md`: prompt architecture reference.
- `release-playbook.md`: release process.
- `releases/`: dated desktop beta and release summaries.
- `test-intents/`: maintained intent-test assets and reports.
- `skills-migration/`: source skill definitions and migration notes; do not archive
  `SKILL.md` files unless the skill is removed from the product.
- `superpowers/specs/`: specs still referenced by source code, tests, or current runtime
  behavior.
- `superpowers/plans/`: active plans or plans directly referenced by tests/source only.
- `src-tauri/prompts/*.md`: product prompts, not documentation clutter.

## Archive Policy

- Move completed plans, dated gap analyses, reviews, handoffs, and superseded specs to
  `docs/archive/YYYY-MM/`.
- Do not archive documents referenced by source files, tests, `CLAUDE.md`, `AGENTS.md`,
  scripts, or bundled resources unless those references are updated in the same change.
- Keep product prompts and skill `SKILL.md` files in their original locations.
