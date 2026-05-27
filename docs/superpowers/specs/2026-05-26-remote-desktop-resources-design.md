# Remote Desktop Resources — Skills, Employees, Expert Teams

**Date**: 2026-05-26
**Status**: Implemented for Phase 1 desktop sync slice
**Scope**: AIjia desktop + Lotus api-gateway / ops-portal / shared models

## 1. Summary

AIjia has three product resource types that should be remotely managed and synchronized to the desktop client:

- Skills: executable knowledge packages, already served through `skill_packages`.
- Digital employee templates: hireable template snapshots, already partially served through `employee_templates`.
- Expert team templates: currently hardcoded in the desktop frontend and not yet served remotely.

The design is to unify the publishing and discovery layer, not the resource content format.

Long term, all three resource types support `public` and `tenant` scopes. Phase 1 only implements platform OPS publishing for `public` resources. Tenant-scoped publishing is reserved in schema and contracts but not exposed in UI or API flows.

All remote employee and expert-team resources must support both Chinese and English for display text and runtime prompts. Existing instances freeze the version they were created with and surface manual upgrade prompts when newer versions are available.

## 1.1 Desktop Implementation Update

The desktop client now treats remote digital employee templates and expert team templates as managed global resources, stored as siblings of the global skills directory:

```text
~/.renlijia/skills
~/.renlijia/employee-templates
~/.renlijia/expert-team-templates
```

On login, the desktop runs best-effort sync for both builtin skills and remote desktop resources. Sync uses the session-key protected Lotus gateway endpoint and passes the current UI language (`zh-CN` or `en-US`) for catalog display projection. A sync failure does not block login; local cache and bootstrap resources remain usable.

Digital employee templates are listed from the local merged catalog in the hire wizard. Remote templates are ready to hire after sync; the desktop does not expose install/uninstall controls for employee templates. Existing hired employee instances remain user data and continue to freeze the exact template snapshot they were created with.

Expert teams are also listed from the local merged catalog with bootstrap fallback. Remote manifests can provide production fields (`stableName`, `agentName`, `name`, `persona`) plus `displayI18n`, `promptI18n`, and `directorPromptI18n`. Language switching remaps display names, examples, composer placeholders, and director prompt templates locally without another network request.

Expert avatars are expected to use a shared OSS atlas when provided by the manifest:

```json
{
  "kind": "atlas",
  "url": "https://lotus-releases.oss-cn-beijing.aliyuncs.com/desktop-resources/expert-team-avatars/v1/avatar-atlas.svg",
  "x": 96,
  "y": 0,
  "w": 96,
  "h": 96,
  "atlasWidth": 672,
  "atlasHeight": 384
}
```

The frontend renders atlas entries through CSS `background-image`, `background-size`, and `background-position`, so all experts in the current atlas share a single browser-cached request. The older packaged SVG avatars remain only as bootstrap fallback for built-in teams.

## 2. Decisions

1. Keep business formats independent.
   Skills remain zip packages with `SKILL.md`. Employee templates remain JSON snapshots. Expert teams become JSON snapshots. Do not force a shared package format.

2. Add a unified desktop resource catalog.
   A new directory model, tentatively `desktop_resources`, owns common publishing, discovery, versioning, scope, visibility, i18n display metadata, manifest pointers, compatibility gates, and sort/featured fields.

3. Phase 1 scope is public resources only.
   OPS can publish official platform resources. The data model reserves `tenant_id` and `scope=tenant`, but tenant upload, tenant review, and tenant override behavior are deferred.

4. Display text and runtime prompts are both bilingual.
   For employee templates and expert teams, `zh-CN` and `en-US` must both be present before publish. The desktop can fallback to `zh-CN` if older data is missing English, but OPS validation should prevent new incomplete publishes.

5. Existing employee instances and expert-team conversations are version-frozen.
   New hires and new expert-team conversations use the latest downloaded snapshot. Existing ones keep their frozen snapshot until the user manually upgrades.

## 3. Goals

- Make skills, digital employees, and expert teams discoverable through one desktop catalog API.
- Let each resource type keep a strict, domain-specific manifest/snapshot.
- Move expert teams out of frontend hardcoded constants.
- Bring digital employees into the same catalog discovery surface as skills.
- Support Chinese/English language switching for catalog UI and runtime behavior.
- Preserve offline behavior through bootstrap resources and local caches.
- Add a clean path for future tenant-scoped resources without implementing tenant publishing in Phase 1.

## 4. Non-Goals

- No unified zip/package format across the three resource types.
- No tenant admin upload UI in Phase 1.
- No automatic upgrade of existing employee instances or expert-team conversations.
- No full redesign of the skill package format.
- No migration of user-created local skills into Lotus.

## 5. Unified Catalog Model

`desktop_resources` is a common index table. It does not store the full skill, employee, or expert-team definition. It stores enough metadata for listing, filtering, version comparison, authorization, and downloading the correct type-specific manifest.

Proposed fields:

```text
id
resource_type              skill | employee_template | expert_team_template
resource_id                stable id within its type
version                    canonical version string
scope                      public | tenant
tenant_id                  0 for public in Phase 1
status                     draft | published | deprecated | archived
display_i18n               JSON object keyed by zh-CN / en-US
category
icon
featured
sort_order
manifest_url
manifest_sha256
manifest_size
min_desktop_version
feature_flags
created_by
published_at
created_at
updated_at
```

Uniqueness:

```text
(resource_type, resource_id, version, scope, tenant_id)
```

Visibility in Phase 1:

```text
scope = public
tenant_id = 0
status = published
min_desktop_version <= current desktop version
```

Long-term visibility:

```text
public published resources
+ tenant published resources for current tenant
+ optional plan / feature-flag filters
```

`display_i18n` is only catalog-level display metadata. It should not become the canonical runtime definition for employees or expert teams.

Example:

```json
{
  "zh-CN": {
    "name": "战略推演团",
    "description": "重大决策前的多视角压力测试",
    "tagline": "重大决策前的多视角压力测试",
    "examples": ["是否拓展东南亚市场"]
  },
  "en-US": {
    "name": "Strategy Simulation Team",
    "description": "Stress-test major decisions from multiple angles",
    "tagline": "Multi-perspective pressure testing before major decisions",
    "examples": ["Should we expand into Southeast Asia?"]
  }
}
```

## 6. Type-Specific Formats

### 6.1 Skills

Skills continue to use the existing `skill_packages` flow:

- OPS upload publishes public skills.
- Package format remains zip with `SKILL.md`.
- Desktop installation and registry reload remain under the current skill sync path.

Phase 1 catalog integration:

- Mirror published public skill rows into `desktop_resources`, or generate catalog rows from `skill_packages` at query time.
- Keep the existing download/install implementation.
- Do not require skill runtime prompt bilingual support in Phase 1. Catalog display can use current DB fields and later evolve with skill-specific i18n.

Reasoning: skills are materially different from employee and expert templates because they may contain references, scripts, and loading instructions. Their content package needs independent hardening.

### 6.2 Digital Employee Templates

Employees already have a remote template shape in Rust (`TemplateSnapshot`) and Lotus (`employee_templates`). Phase 1 should strengthen it and attach it to the unified catalog.

Required snapshot shape additions:

```text
display_i18n
prompt_i18n
schema_i18n
```

`display_i18n` includes name, role, description, badge, examples, and catalog-facing text.

`prompt_i18n` includes `system_prompt_extra` and any dispatch guidance that affects runtime behavior.

`schema_i18n` localizes resource config form labels, descriptions, placeholders, help text, and option labels.

Existing fields such as `tool_whitelist`, `cron`, `default_skill_id`, `skill_ids`, `requires_attachment`, and `resource_config_schema` remain type-specific fields.

Hiring behavior:

- The hire wizard lists latest available employee templates in the current UI language.
- On hire, the desktop writes the exact template snapshot into the employee instance directory.
- Employee dispatch reads the frozen snapshot.
- Current language at dispatch selects `prompt_i18n[language]` for runtime prompt text.

Upgrade behavior:

- If the catalog/cache has a newer version for the same `template_id`, the employee drawer shows an upgrade prompt.
- The user confirms upgrade.
- The employee is not currently running.
- The instance snapshot is replaced with the selected newer snapshot.
- Existing conversations and inbox entries remain unchanged.

### 6.3 Expert Team Templates

Expert teams move from frontend constants to versioned JSON snapshots.

Proposed `ExpertTeamSnapshot`:

```json
{
  "teamId": "strategy",
  "version": "1.0.0",
  "facilitationStyle": "rounds",
  "displayI18n": {
    "zh-CN": {
      "name": "战略推演团",
      "tagline": "重大决策前的多视角压力测试",
      "examples": ["是否拓展东南亚市场"],
      "composerPlaceholder": "告诉他们你想推演什么决策..."
    },
    "en-US": {
      "name": "Strategy Simulation Team",
      "tagline": "Multi-perspective pressure testing before major decisions",
      "examples": ["Should we expand into Southeast Asia?"],
      "composerPlaceholder": "Tell the team what decision you want to test..."
    }
  },
  "experts": [
    {
      "stableName": "cfo",
      "emoji": "💰",
      "displayI18n": {
        "zh-CN": { "name": "CFO" },
        "en-US": { "name": "CFO" }
      },
      "promptI18n": {
        "zh-CN": { "persona": "关注 ROI、现金流、风险敞口" },
        "en-US": { "persona": "Focuses on ROI, cash flow, and risk exposure" }
      }
    }
  ],
  "directorPromptI18n": {
    "zh-CN": {
      "template": "你现在的任务是为用户主持一场「{teamName}」圆桌讨论..."
    },
    "en-US": {
      "template": "Your task is to facilitate a {teamName} roundtable..."
    }
  }
}
```

Important constraints:

- `teamId` is stable and language-independent.
- Expert `stableName` is stable and language-independent.
- Runtime spawn/name identity must use `stableName`, not localized display names.
- Localized expert names are UI labels only.
- `directorPromptI18n` is runtime content, not just display text.
- Published snapshots are immutable; new behavior requires a new version.

Conversation behavior:

- When a user starts a team conversation, the desktop freezes the selected `ExpertTeamSnapshot` into the conversation directory.
- The conversation source keeps `expertTeamId`, `version`, and source metadata.
- New turns in that conversation render director prompts from the frozen snapshot.
- Language switching can change UI labels because both languages remain in the frozen snapshot.
- A turn already in progress keeps the language selected when it was submitted.

Upgrade behavior:

- If a newer snapshot exists for the same `teamId`, the expert-team banner or team page shows an upgrade prompt.
- The user confirms upgrade.
- No turn is currently running in that conversation.
- The conversation snapshot is replaced.
- Existing messages and transcripts remain unchanged.

## 7. Server Architecture

### 7.1 Shared Model

Add `shared/model/desktop_resource.go` for the catalog row. AutoMigrate should create the table and indexes.

The catalog is intentionally separate from existing type-specific tables. It allows unified discovery without constraining type evolution.

### 7.2 OPS Portal

Phase 1 OPS responsibilities:

- Publish official public skills, employees, and expert teams.
- Validate bilingual fields for employee and expert-team resources.
- Generate canonical snapshots.
- Upload snapshots/packages to OSS.
- Write/update `desktop_resources` rows.

For skills, the current skill marketplace can remain the primary editing UI. Publishing a public skill should create or refresh its `desktop_resources` mirror.

For employee templates, the existing OPS employee-template management should be extended:

- Add bilingual display and prompt fields.
- On publish, upload the canonical employee snapshot and update `desktop_resources`.

For expert teams, add a new OPS management surface:

- Create draft.
- Edit bilingual display.
- Edit experts and stable names.
- Edit bilingual personas.
- Edit bilingual director prompt templates.
- Publish immutable version.
- Deprecate or archive.

Strict publish validation:

- `zh-CN` and `en-US` required for display fields.
- `zh-CN` and `en-US` required for runtime prompt fields.
- `stableName` must match a conservative ASCII identifier pattern such as `^[a-z][a-z0-9-]{1,63}$`.
- `resource_id + version + scope + tenant_id + resource_type` must be unique.
- Canonical JSON hash must match `manifest_sha256`.
- Published content fields are immutable.

### 7.3 API Gateway

Add a session-key protected catalog endpoint:

```text
GET /v1/desktop-resources?types=skill,employee_template,expert_team_template&lang=zh-CN
```

The response contains visible catalog rows and signed manifest/package URLs where needed.

Example:

```json
{
  "data": [
    {
      "resourceType": "expert_team_template",
      "resourceId": "strategy",
      "version": "1.0.0",
      "scope": "public",
      "display": {
        "name": "战略推演团",
        "description": "重大决策前的多视角压力测试"
      },
      "manifestUrl": "https://...",
      "manifestSha256": "abc...",
      "manifestSize": 10240,
      "minDesktopVersion": "0.5.31"
    }
  ]
}
```

Use authenticated gateway access even for public resources. This keeps room for tenant visibility, plan gating, feature flags, and rollout controls without changing the desktop contract later.

## 8. Desktop Architecture

Add a Rust catalog/sync layer, but keep handlers type-specific:

```text
src-tauri/src/runtime/desktop_resources/
  catalog.rs
  sync.rs
  cache.rs
  handlers/
    skill.rs
    employee.rs
    expert_team.rs
```

Responsibilities:

- Fetch `/v1/desktop-resources`.
- Cache catalog index under `~/.renlijia/global/desktop-resources/index.json`.
- Compare current cached versions.
- Dispatch changed resources to type-specific handlers.
- Record per-resource sync results and diagnostics.

Type-specific caches:

```text
~/.renlijia/skills/                                      # existing skill install root
~/.renlijia/employee-templates-cache/{templateId}/...    # existing public employee cache
~/.renlijia/expert-team-templates-cache/{teamId}/...     # new public expert team cache
users/{scope}/employees/{id}/template/template.json      # frozen employee instance
users/{scope}/conversations/{id}/expert-team/template.json # frozen team conversation
```

Phase 1 uses public caches only for downloaded catalog resources. The user-scoped frozen instance/conversation snapshots are still required because upgrade is manual and per instance.

Frontend-facing IPC:

```text
sync_desktop_resources(types?: string[])
get_desktop_resource_status()
employee_template_catalog()
expert_team_template_catalog()
employee_template_upgrade(employeeId, targetVersion)
expert_team_upgrade_conversation(conversationId, targetVersion)
```

Existing `employee_template_catalog()` can stay as the public API and return localized display fields based on current language, while retaining full bilingual snapshot data internally.

`EXPERT_TEAMS` should become bootstrap fallback data, not the authoritative source.

## 9. i18n Runtime Semantics

Language sources:

- UI language comes from the existing frontend i18n setting (`zh-CN` / `en-US`).
- Runtime prompt language is captured when the user submits the employee run or expert-team turn.
- Snapshot files store both languages so UI can switch after creation.

Selection rules:

```text
display = display_i18n[currentLanguage] ?? display_i18n["zh-CN"]
prompt  = prompt_i18n[currentLanguage]  ?? prompt_i18n["zh-CN"]
```

Fallback is a desktop resilience behavior, not a publishing policy. OPS publish validation must require both languages for new employee and expert-team resources.

One turn uses one runtime language. If the user changes UI language while a turn is streaming, the current prompt does not change mid-run.

## 10. Offline and Failure Behavior

- If catalog sync fails, keep using existing cache and bootstrap resources.
- If a single resource download fails, skip that resource and continue syncing the rest.
- If sha256 verification fails, discard the downloaded artifact and keep the previous cached version.
- If a resource requires a newer desktop version, hide it from create flows and optionally show it as unavailable in diagnostics.
- If no remote expert-team catalog is available, use bootstrap expert teams derived from the current hardcoded list.
- Login should not fail because resource sync failed.

## 11. Rollout Plan

Phase 1:

1. Add `desktop_resources` model/table and gateway catalog endpoint.
2. Mirror existing public skills and public employee templates into the catalog.
3. Add i18n fields to employee template publish flow and snapshots.
4. Add expert-team model, OPS publish flow, snapshot format, and catalog rows.
5. Add desktop resource sync and expert-team cache/freeze logic.
6. Update frontend expert-team page to read remote/bootstrap catalog.
7. Add manual upgrade prompts for employee instances and expert-team conversations.

Phase 2:

- Improve OPS editing UX.
- Add resource status diagnostics and sync history.
- Add staged rollout / feature flags / min version filters.

Phase 3:

- Add tenant-scoped publishing and tenant catalog merge.
- Define tenant vs public conflict behavior.
- Add review workflow if tenant resources can become public.

## 12. Testing

Go tests:

- Catalog endpoint returns only `published public` resources in Phase 1.
- `resource_type/resource_id/version/scope/tenant_id` uniqueness is enforced.
- Published resource content cannot be mutated.
- Employee and expert-team publish fails when either language is missing.
- Expert `stableName` validation rejects localized or invalid names.
- Canonical snapshot sha256 is stable.

Rust tests:

- Catalog sync keeps previous cache when network fails.
- sha256 mismatch discards artifact.
- Employee hire freezes the selected version.
- Expert-team conversation freezes the selected version.
- Newer versions are detected.
- Running employee/team turn blocks upgrade.
- Language fallback uses `zh-CN` only when the requested language is missing.

TypeScript/Vitest:

- Expert-team page renders from remote catalog.
- Bootstrap fallback renders when IPC fails.
- Language switch changes display text.
- Director prompt uses English template under `en-US`.
- Existing conversation continues to use frozen snapshot after catalog changes.

Regression:

- Existing skill sync and registry reload still work.
- Existing employee hire and dispatch still work with bootstrap data.
- Existing expert team flow works when only bootstrap data is available.

## 13. Open Risks and Mitigations

- Risk: unified catalog becomes a dumping ground for type-specific details.
  Mitigation: catalog stores only discovery metadata and manifest pointers; type-specific content stays in type modules.

- Risk: English UI still triggers Chinese runtime behavior.
  Mitigation: OPS validation requires bilingual runtime prompts for employees and expert teams.

- Risk: expert-team identity breaks when names are translated.
  Mitigation: use stable ASCII `stableName` for runtime identity and localized names only for display.

- Risk: resource sync failure blocks login.
  Mitigation: sync is best-effort; cached/bootstrap resources remain valid.

- Risk: Phase 1 schema overfits public-only resources.
  Mitigation: keep `scope` and `tenant_id` in the catalog key from day one, even while tenant publishing remains disabled.
