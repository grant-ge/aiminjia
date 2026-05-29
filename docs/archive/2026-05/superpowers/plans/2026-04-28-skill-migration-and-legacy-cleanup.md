# Skill Migration and Legacy Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将旧 `src-tauri/plugins/*` 中的非 `skill-smith` 技能迁移为符合 `AIjia SKILL.md` 最新规范的可复制 skill 包，并删除旧 `plugin.toml` / `workflow.toml` / `skill_smith` 入口。

**Architecture:** 迁移产物放在 `docs/skills-migration/{skill-id}/`，每个目录以 `SKILL.md` 为唯一入口，按需携带 `references/knowledge/*` 与 `scripts/legacy-precompute/*`。迁移必须由子 agent 按技能分组人工阅读旧 `plugin.toml`、`workflow.toml`、`prompts/*.md`、`scripts/knowledge/*` 后改写，不使用批量脚本生成最终内容；主 agent 只负责任务分配、冲突集成、审查、验证和旧入口清理。

**Tech Stack:** Markdown `SKILL.md` + YAML frontmatter，Rust/Tauri SKILL.md parser 测试，React/TypeScript 前端入口清理，Cargo/Vitest/静态校验。

---

## Confirmed Scope

- 迁移来源：`src-tauri/plugins/*` 中除 `skill-smith` 外的 22 个旧技能目录。
- 不迁移：`src-tauri/plugins/skill-smith`，它是旧 `plugin.toml + workflow.toml` 创建器，按用户确认直接移除。
- 迁移目标：`docs/skills-migration/{skill-id}/`。
- 新规范依据：`docs/superpowers/specs/2026-04-28-aijia-skill-spec.md`。
- 删除目标：旧 `src-tauri/plugins/` 资源目录、`skill_smith` 后端命令/LLM 工具包装、前端草稿恢复入口、Tauri bundle resources 中的 `plugins` 映射。
- 保留目标：`src-tauri/src/plugin/skill/*`、`src-tauri/src/runtime/tools/builtin/load_skill.rs`、`src-tauri/src/commands/plugin.rs`、`src-tauri/src/commands/skill_management.rs::list_skills_from_registry`。

## Required Skill Package Shape

每个迁移后的 skill 包必须满足：

```text
docs/skills-migration/{skill-id}/
├── SKILL.md
├── references/knowledge/*.json        # 仅当旧 skill 有知识库时保留
├── scripts/legacy-precompute/*.py     # 仅当旧 skill 有可复用 precompute 脚本时保留
└── migration-notes.md                 # 说明旧来源、迁移取舍、复核点
```

`SKILL.md` frontmatter 必须包含：

```yaml
---
name: {skill-id}
description: <LLM 看到的何时使用说明>
when_to_use: <触发场景、关键词、是否需要文件>
allowed-tools:
  - load_file
  - execute_python
  - export_data
  - generate_report
  - generate_slides
model: opus
effort: high
context: inline
user-invocable: true
disable-model-invocation: false
version: "1.0"
metadata:
  label: <中文显示名>
---
```

`allowed-tools` 只列该技能真实会用到的工具；不需要的工具不要为了统一而加入。

---

## Task 1: Lock Migration Contract and Audit Tests

**Files:**
- Modify: `src-tauri/tests/review_skill_system_no_legacy_test.rs`
- Create/Modify: `docs/superpowers/plans/2026-04-28-skill-migration-and-legacy-cleanup.md`

- [ ] **Step 1: 收紧旧格式审计规则**

在 `review_skill_system_no_legacy_test.rs` 中移除 `commands/skill_smith/` 与 `commands/skill_management.rs` 的跳过例外，仅保留 `storage/migration.rs` 历史迁移 fixture 例外。

- [ ] **Step 2: 运行 RED/确认现状**

Run:

```bash
cd src-tauri && cargo test --test review_skill_system_no_legacy_test -- --nocapture
```

Expected before cleanup: 若旧入口仍存在，应 FAIL 并指出 `plugin.toml` / `workflow.toml` / `skill_smith` 残留；若前置清理已发生，应 PASS。

- [ ] **Step 3: 记录计划边界**

确认本计划明确要求：迁移 Task 2 必须由子 agent 分组人工执行，不得使用脚本批量生成最终 `SKILL.md`。

---

## Task 2: Subagent-Driven Skill Package Migration

**Files:**
- Create/Modify only under: `docs/skills-migration/**`
- Read-only source: `src-tauri/plugins/**`
- Do not modify: `src-tauri/src/**`, `src/**`, `src-tauri/tauri.conf.json`

**Subagent allocation:** 主 agent 分 5 个 worker 子 agent，每个 worker 只写自己的 skill 子目录，避免写集冲突。

**Source recovery note:** 如果执行本任务时 `src-tauri/plugins/**` 已在工作树中被删除，worker 不得恢复旧目录；应通过 git 读取旧源，例如 `git show HEAD:src-tauri/plugins/{skill-id}/plugin.toml`、`git show HEAD:src-tauri/plugins/{skill-id}/workflow.toml`、`git ls-tree -r --name-only HEAD src-tauri/plugins/{skill-id}`。

### Worker A: Writing and Document Skills

**Owned skills:**
- `biz-writing`
- `biz-proposal`
- `multi-file-handler`
- `okr-coach`

**Write set:**
- `docs/skills-migration/biz-writing/**`
- `docs/skills-migration/biz-proposal/**`
- `docs/skills-migration/multi-file-handler/**`
- `docs/skills-migration/okr-coach/**`

- [ ] **Step 1: Worker A reads old source files**

Read each owned skill's `plugin.toml`, `workflow.toml`, all `prompts/*.md`, and `scripts/knowledge/*` if present.

- [ ] **Step 2: Worker A rewrites final packages**

For each owned skill, create/update `SKILL.md`, copy useful knowledge JSON to `references/knowledge/`, copy reusable scripts to `scripts/legacy-precompute/`, and write `migration-notes.md`.

- [ ] **Step 3: Worker A self-checks**

Each `SKILL.md` must be readable as standalone instructions and must not mention old stateful runtime concepts as active mechanisms (`plugin.toml`, `workflow.toml`, `[precompute_result]` as system-provided state). Historical notes may mention old paths only in `migration-notes.md`.

### Worker B: Finance and Business Analysis Skills

**Owned skills:**
- `budget-analysis`
- `finance-analysis`
- `sales-analysis`
- `ops-analysis`

**Write set:**
- `docs/skills-migration/budget-analysis/**`
- `docs/skills-migration/finance-analysis/**`
- `docs/skills-migration/sales-analysis/**`
- `docs/skills-migration/ops-analysis/**`

Repeat Worker A steps for owned skills.

### Worker C: Compensation and HR Analytics Skills

**Owned skills:**
- `salary-benchmarking`
- `comp-analysis-v2`
- `talent-9box`
- `recruitment-funnel`

**Write set:**
- `docs/skills-migration/salary-benchmarking/**`
- `docs/skills-migration/comp-analysis-v2/**`
- `docs/skills-migration/talent-9box/**`
- `docs/skills-migration/recruitment-funnel/**`

Repeat Worker A steps for owned skills.

### Worker D: Organization and People System Skills

**Owned skills:**
- `org-diagnosis`
- `pa-maturity`
- `perf-system-design`
- `engagement-survey`

**Write set:**
- `docs/skills-migration/org-diagnosis/**`
- `docs/skills-migration/pa-maturity/**`
- `docs/skills-migration/perf-system-design/**`
- `docs/skills-migration/engagement-survey/**`

Repeat Worker A steps for owned skills.

### Worker E: Compliance, Contract, Customer/User Skills

**Owned skills:**
- `contract-review`
- `labor-compliance`
- `policy-compliance-audit`
- `customer-segmentation`
- `survey-analysis`
- `user-behavior`

**Write set:**
- `docs/skills-migration/contract-review/**`
- `docs/skills-migration/labor-compliance/**`
- `docs/skills-migration/policy-compliance-audit/**`
- `docs/skills-migration/customer-segmentation/**`
- `docs/skills-migration/survey-analysis/**`
- `docs/skills-migration/user-behavior/**`

Repeat Worker A steps for owned skills.

- [ ] **Step 4: Main agent validates migration shape**

Run a lightweight validation that only checks structure and YAML parse, not content generation:

```bash
python3 - <<'PY'
from pathlib import Path
import sys, yaml
root = Path('docs/skills-migration')
expected = {
  'biz-writing','biz-proposal','multi-file-handler','okr-coach',
  'budget-analysis','finance-analysis','sales-analysis','ops-analysis',
  'salary-benchmarking','comp-analysis-v2','talent-9box','recruitment-funnel',
  'org-diagnosis','pa-maturity','perf-system-design','engagement-survey',
  'contract-review','labor-compliance','policy-compliance-audit',
  'customer-segmentation','survey-analysis','user-behavior',
}
actual = {p.name for p in root.iterdir() if p.is_dir()}
missing = expected - actual
extra = actual - expected
errors = []
if missing: errors.append(f'missing: {sorted(missing)}')
if extra: errors.append(f'extra: {sorted(extra)}')
for skill_id in sorted(expected):
    p = root / skill_id / 'SKILL.md'
    if not p.exists():
        errors.append(f'{skill_id}: missing SKILL.md')
        continue
    text = p.read_text()
    if not text.startswith('---\n'):
        errors.append(f'{skill_id}: missing YAML frontmatter')
        continue
    try:
        _, fm, body = text.split('---', 2)
        data = yaml.safe_load(fm) or {}
    except Exception as exc:
        errors.append(f'{skill_id}: invalid YAML: {exc}')
        continue
    if data.get('name') != skill_id:
        errors.append(f'{skill_id}: frontmatter name mismatch: {data.get("name")}')
    if not data.get('description'):
        errors.append(f'{skill_id}: missing description')
    if not body.strip():
        errors.append(f'{skill_id}: empty body')
if errors:
    print('\n'.join(errors))
    sys.exit(1)
print('skill migration package shape OK')
PY
```

Expected: PASS.

---

## Task 3: Remove Legacy Plugin Resources and Skill-Smith Entrypoints

**Files:**
- Delete: `src-tauri/plugins/**`
- Delete: `src-tauri/src/commands/skill_smith/**`
- Delete: `src-tauri/src/llm/tool_executor/skill_smith.rs`
- Delete: `src-tauri/src/plugin/builtin/tools/skill_smith_*.rs`
- Delete: `src/components/skill-smith/**`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/llm/tool_executor/mod.rs`
- Modify: `src-tauri/src/plugin/builtin/tools/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/tauri.ts`
- Modify: `src/components/chat/WelcomeScreen.tsx`
- Modify: `src/components/settings/SkillsTab.tsx`
- Modify: `src/features/skill-center/SkillCenterPage.tsx`
- Modify: `src/features/skill-center/SkillCenterPage.integration.test.tsx`
- Modify: `src/stores/skillStore.ts`
- Modify: `src/data/home-suggestions.ts`

- [ ] **Step 1: Delete old bundled plugin resources**

Remove `src-tauri/plugins/` entirely after Task 2 migration is reviewed.

- [ ] **Step 2: Remove Tauri bundle resource mapping**

In `src-tauri/tauri.conf.json`, remove only this resource entry:

```json
"plugins": "plugins"
```

Keep `playwright-runtime` and `prompts`.

- [ ] **Step 3: Remove Rust module and invoke registrations**

Remove `pub mod skill_smith;`, remove `tool_executor::skill_smith`, remove `plugin/builtin/tools/skill_smith_*` module exports, remove startup draft cleanup, and remove all `commands::skill_smith::*` handlers from `generate_handler!`.

- [ ] **Step 4: Remove frontend draft APIs and UI**

Delete `DraftResumeBanner`; remove imports/usages from `WelcomeScreen` and `SkillsTab`; remove skill-smith draft types/functions from `src/lib/tauri.ts`.

- [ ] **Step 5: Replace create-skill entry**

In skill center, replace the old `createConversationFromSkill('skill-smith')` CTA with an import/upload action. Update the integration test to assert it opens upload/import UI and does not call `createConversationFromSkill`.

- [ ] **Step 6: Remove skill-smith recommendation IDs**

Remove `skill-smith` from `RECOMMENDED_SKILL_IDS` and home suggestions. Use existing non-removed skill IDs or generic prompt-only behavior.

---

## Task 4: Verification and Review

**Files:**
- No new production files expected.

- [ ] **Step 1: Rust compile check**

Run:

```bash
cd src-tauri && cargo check --all-targets
```

Expected: exit 0. Warnings may remain if unrelated existing warnings are present.

- [ ] **Step 2: Skill runtime focused tests**

Run:

```bash
cd src-tauri && cargo test --test skill_md_frontmatter_test -- --nocapture
cd src-tauri && cargo test --test skill_md_loader_test -- --nocapture
cd src-tauri && cargo test --test skill_md_catalog_test -- --nocapture
cd src-tauri && cargo test --test list_skills_returns_skill_md_only_test -- --nocapture
cd src-tauri && cargo test --test review_skill_system_no_legacy_test -- --nocapture
```

Expected: all exit 0.

- [ ] **Step 3: Frontend focused tests**

Run if dependencies are installed:

```bash
pnpm test -- src/features/skill-center/SkillCenterPage.integration.test.tsx src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx
```

Expected: exit 0. If `node_modules` is missing, report the exact failure and do not claim frontend tests passed.

- [ ] **Step 4: Static grep audit**

Run:

```bash
rg -n "skill_smith|skill-smith|src-tauri/plugins|\"plugins\"\s*:\s*\"plugins\"" src-tauri/src src src-tauri/tauri.conf.json
rg -n "plugin\.toml|workflow\.toml|PluginManifest|WorkflowManifest" src-tauri/src src
```

Expected: no production hits except explicitly accepted historical migration fixtures under `src-tauri/src/storage/migration.rs` when scanning that path.

- [ ] **Step 5: Final review**

Request independent review for:

- migration completeness of `docs/skills-migration/**`
- absence of old runtime entrypoints
- no accidental deletion of new `plugin/skill/*` or `load_skill` chain

---

## Self-Review Notes

- Spec coverage: covers user-confirmed docs path, no `skill-smith` migration, old plugins deletion, subagent-only migration, and verification.
- Placeholder scan: no `TBD`/`TODO` implementation placeholders are used as task content.
- Type consistency: uses existing `SkillInfo`, `SKILL.md`, `load_skill`, and `list_skills_from_registry` terminology from the current codebase.
