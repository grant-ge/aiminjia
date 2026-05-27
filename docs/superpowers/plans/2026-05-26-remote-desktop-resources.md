# Remote Desktop Resources Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build Phase 1 of remote desktop resources: a unified public OPS catalog for skills, employee templates, and expert-team templates, while keeping each resource format independent.

**Architecture:** Lotus owns a `desktop_resources` catalog row for common discovery and version metadata; each resource type keeps its own package/snapshot format. The desktop sync layer fetches the catalog through the gateway, writes a resilient local index, and dispatches artifacts to type-specific caches. Expert teams become versioned snapshots and freeze into conversations; employee templates keep their existing snapshot model and gain bilingual display/runtime fields.

**Tech Stack:** Go 1.23 + Gin + GORM + MySQL + Redis in `~/lotus`; Tauri 2 + Rust + reqwest + serde in `aijia/code/src-tauri`; React 19 + Zustand + Vitest in `aijia/code/src`.

## Implementation Update — 2026-05-26

Desktop follow-up for remote employee templates and expert teams is implemented in the `feat/remote-desktop-resources` worktree:

- Login now triggers best-effort `sync_desktop_resources` alongside builtin skill sync.
- Employee templates and expert team templates are stored under `~/.renlijia/employee-templates` and `~/.renlijia/expert-team-templates`, sibling to global `~/.renlijia/skills`.
- The hire wizard syncs through the unified desktop resource gateway and maps localized employee template display/prompt fields for the current UI language.
- Expert team catalog rendering supports production manifest persona fields, director prompt templates with `{topic}` and `{{topic}}`, local language remapping, dynamic remote ids/labels, and OSS avatar atlas rendering.
- Remote atlas avatars use one shared cached URL and CSS background positioning; packaged SVG avatars remain bootstrap fallback.

Verification commands run for this desktop slice:

```bash
npm run test -- src/features/expert-teams/__tests__/buildDirectorPrompt.test.ts src/features/expert-teams/__tests__/useExpertTeamCatalog.test.tsx src/features/expert-teams/expertVisuals.test.ts src/features/auth/AuthGate.integration.test.tsx src/features/employees/templates.test.ts src/components/chat-scene/ExpertTeamWelcome.test.tsx
npm run build
cargo test --lib storage::aijia_home::tests::managed_resource_dirs_are_siblings_of_global_skills_dir
cargo test --lib runtime::expert_team::store
cargo test --lib runtime::employee::template_store
cargo test --lib runtime::desktop_resources::sync
```

---

## Scope Check

This spec spans multiple subsystems. Execute it as sequential Phase 1 slices:

1. Lotus shared catalog and gateway read API.
2. OPS mirror/publish integration for existing skills and employee templates.
3. Expert-team template service model and public OPS publishing.
4. Desktop catalog sync and cache layer.
5. Desktop expert-team snapshots and conversation freeze.
6. Frontend expert-team remote catalog and i18n prompt rendering.
7. Manual upgrade affordances for employee instances and expert-team conversations.

Each task below is meant to be reviewable and testable on its own.

## File Map

Lotus shared/server files:

- Create `/Users/gezhigang/lotus/code/shared/model/desktop_resource.go`: catalog row shared by gateway and OPS.
- Modify `/Users/gezhigang/lotus/code/shared/migration/migrate.go`: include `DesktopResource` in AutoMigrate and add uniqueness/index hardening.
- Create `/Users/gezhigang/lotus/code/shared/pkg/desktopresource/upsert.go`: small helper that writes catalog mirror rows from type-specific publish handlers.
- Create `/Users/gezhigang/lotus/code/api-gateway/internal/handler/desktop_resources.go`: session-key protected catalog endpoint.
- Create `/Users/gezhigang/lotus/code/api-gateway/internal/handler/desktop_resources_test.go`: public-only filtering and language projection tests.
- Modify `/Users/gezhigang/lotus/code/api-gateway/cmd/server/main.go`: register `/v1/desktop-resources`.
- Modify `/Users/gezhigang/lotus/code/ops-portal/server/internal/handler/skill_marketplace.go`: mirror public skill publishes/metadata changes into `desktop_resources`.
- Modify `/Users/gezhigang/lotus/code/ops-portal/server/internal/handler/employee_template.go`: add bilingual fields to snapshots and mirror published public templates into `desktop_resources`.
- Create `/Users/gezhigang/lotus/code/shared/model/expert_team_template.go`: versioned expert-team template model.
- Create `/Users/gezhigang/lotus/code/ops-portal/server/internal/handler/expert_team_template.go`: OPS CRUD/publish for public expert teams.
- Modify `/Users/gezhigang/lotus/code/ops-portal/server/cmd/server/main.go`: register expert-team OPS routes.

Desktop Rust files:

- Create `src-tauri/src/runtime/desktop_resources/mod.rs`: module exports.
- Create `src-tauri/src/runtime/desktop_resources/catalog.rs`: catalog item types, language projection, version comparison.
- Create `src-tauri/src/runtime/desktop_resources/sync.rs`: gateway fetch and local index write.
- Create `src-tauri/src/runtime/expert_team/mod.rs`: module exports.
- Create `src-tauri/src/runtime/expert_team/store.rs`: `ExpertTeamSnapshot`, bootstrap/cache/freeze helpers.
- Create `src-tauri/src/runtime/expert_team/expert_teams_bootstrap.json`: current eight built-in teams in bilingual-capable shape.
- Modify `src-tauri/src/storage/aijia_home.rs`: add `expert_team_templates_cache_dir`.
- Modify `src-tauri/src/lib.rs`: register new modules and Tauri commands.
- Create `src-tauri/src/commands/desktop_resources.rs`: `sync_desktop_resources`, `get_desktop_resource_status`.
- Create `src-tauri/src/commands/expert_teams.rs`: `expert_team_template_catalog`, `expert_team_upgrade_conversation`.
- Modify `src-tauri/src/runtime/employee/template_store.rs`: add bilingual fields and upgrade detection helpers.

Frontend TypeScript files:

- Modify `src/lib/tauri.ts`: add IPC types/wrappers for desktop resources and expert-team snapshots.
- Modify `src/features/expert-teams/teams.ts`: convert hardcoded list to bootstrap fallback and runtime-compatible type.
- Create `src/features/expert-teams/useExpertTeamCatalog.ts`: load remote catalog with bootstrap fallback.
- Modify `src/features/expert-teams/ExpertTeamsPage.tsx`: render remote catalog.
- Modify `src/features/expert-teams/expertTeamRegistry.ts`: allow dynamic team ids and labels.
- Modify `src/features/expert-teams/buildDirectorPrompt.ts`: render from `directorPromptI18n`.
- Modify `src/components/chat-scene/ChatBottomArea.tsx`: use frozen expert-team snapshot when composing prompt.
- Modify `src/components/chat-scene/ExpertTeamWelcome.tsx`: use snapshot-aware prompt previews.
- Modify `src/i18n/zh-CN.json` and `src/i18n/en-US.json`: resource sync and upgrade UI strings.

## Task 1: Lotus Shared `DesktopResource` Model

**Files:**
- Create: `/Users/gezhigang/lotus/code/shared/model/desktop_resource.go`
- Modify: `/Users/gezhigang/lotus/code/shared/migration/migrate.go`
- Test: `/Users/gezhigang/lotus/code/shared/model/desktop_resource_test.go`

- [ ] **Step 1: Write model tests**

Create `/Users/gezhigang/lotus/code/shared/model/desktop_resource_test.go`:

```go
package model

import "testing"

func TestDesktopResourceTableName(t *testing.T) {
	if got := (DesktopResource{}).TableName(); got != "desktop_resources" {
		t.Fatalf("TableName() = %q, want desktop_resources", got)
	}
}

func TestDesktopResourceStatusConstants(t *testing.T) {
	statuses := []string{
		DesktopResourceStatusDraft,
		DesktopResourceStatusPublished,
		DesktopResourceStatusDeprecated,
		DesktopResourceStatusArchived,
	}
	want := []string{"draft", "published", "deprecated", "archived"}
	for i := range want {
		if statuses[i] != want[i] {
			t.Fatalf("status[%d] = %q, want %q", i, statuses[i], want[i])
		}
	}
}

func TestDesktopResourceTypeConstants(t *testing.T) {
	types := []string{
		DesktopResourceTypeSkill,
		DesktopResourceTypeEmployeeTemplate,
		DesktopResourceTypeExpertTeamTemplate,
	}
	want := []string{"skill", "employee_template", "expert_team_template"}
	for i := range want {
		if types[i] != want[i] {
			t.Fatalf("type[%d] = %q, want %q", i, types[i], want[i])
		}
	}
}
```

- [ ] **Step 2: Run the failing shared model tests**

Run:

```bash
cd /Users/gezhigang/lotus/code/shared
go test ./model -run TestDesktopResource -count=1
```

Expected: FAIL because `DesktopResource` is undefined.

- [ ] **Step 3: Add the shared model**

Create `/Users/gezhigang/lotus/code/shared/model/desktop_resource.go`:

```go
package model

import "time"

const (
	DesktopResourceTypeSkill              = "skill"
	DesktopResourceTypeEmployeeTemplate   = "employee_template"
	DesktopResourceTypeExpertTeamTemplate = "expert_team_template"

	DesktopResourceScopePublic = "public"
	DesktopResourceScopeTenant = "tenant"

	DesktopResourceStatusDraft      = "draft"
	DesktopResourceStatusPublished  = "published"
	DesktopResourceStatusDeprecated = "deprecated"
	DesktopResourceStatusArchived   = "archived"
)

// DesktopResource is the unified discovery row for desktop-delivered resources.
// It intentionally stores catalog metadata and artifact pointers only; the
// resource body remains owned by the type-specific table/package.
type DesktopResource struct {
	ID            uint64  `gorm:"primaryKey;autoIncrement" json:"id"`
	ResourceType  string  `gorm:"type:varchar(40);not null;uniqueIndex:uk_desktop_resource_version,priority:1;index:idx_desktop_resource_visible" json:"resource_type"`
	ResourceID    string  `gorm:"type:varchar(100);not null;uniqueIndex:uk_desktop_resource_version,priority:2" json:"resource_id"`
	Version       string  `gorm:"type:varchar(32);not null;uniqueIndex:uk_desktop_resource_version,priority:3" json:"version"`
	Scope         string  `gorm:"type:varchar(20);not null;default:'public';uniqueIndex:uk_desktop_resource_version,priority:4;index:idx_desktop_resource_visible" json:"scope"`
	TenantID      uint64  `gorm:"not null;default:0;uniqueIndex:uk_desktop_resource_version,priority:5;index:idx_desktop_resource_visible" json:"tenant_id"`
	Status        string  `gorm:"type:varchar(20);not null;default:'draft';index:idx_desktop_resource_visible" json:"status"`
	DisplayI18n   JSONRaw `gorm:"type:text" json:"display_i18n"`
	Category      string  `gorm:"type:varchar(50)" json:"category"`
	Icon          string  `gorm:"type:varchar(32)" json:"icon"`
	Featured      bool    `gorm:"default:false;index:idx_desktop_resource_sort" json:"featured"`
	SortOrder     int     `gorm:"default:0;index:idx_desktop_resource_sort" json:"sort_order"`
	ManifestURL   string  `gorm:"type:varchar(500)" json:"manifest_url"`
	ManifestSHA256 string `gorm:"column:manifest_sha256;type:varchar(64)" json:"manifest_sha256"`
	ManifestSize  int64   `gorm:"default:0" json:"manifest_size"`
	MinDesktopVer string  `gorm:"type:varchar(32)" json:"min_desktop_version"`
	FeatureFlags  JSONRaw `gorm:"type:text" json:"feature_flags"`
	CreatedBy     uint64  `gorm:"default:0" json:"created_by"`
	PublishedAt   *time.Time `json:"published_at,omitempty"`
	CreatedAt     time.Time  `gorm:"autoCreateTime" json:"created_at"`
	UpdatedAt     time.Time  `gorm:"autoUpdateTime" json:"updated_at"`
}

func (DesktopResource) TableName() string { return "desktop_resources" }
```

- [ ] **Step 4: Add the model to AutoMigrate**

In `/Users/gezhigang/lotus/code/shared/migration/migrate.go`, add `&model.DesktopResource{},` after `&model.EmployeeTemplate{},` in the `models := []interface{}{...}` list.

- [ ] **Step 5: Run shared tests**

Run:

```bash
cd /Users/gezhigang/lotus/code/shared
go test ./model -run TestDesktopResource -count=1
go test ./migration -run TestDoesNotExist -count=1
```

Expected:

- First command: PASS.
- Second command may report `[no test files]`; that is acceptable. It verifies the migration package still compiles.

- [ ] **Step 6: Commit**

```bash
cd /Users/gezhigang/lotus
git add code/shared/model/desktop_resource.go code/shared/model/desktop_resource_test.go code/shared/migration/migrate.go
git commit -m "feat: add desktop resource catalog model"
```

## Task 2: Lotus Catalog Upsert Helper

**Files:**
- Create: `/Users/gezhigang/lotus/code/shared/pkg/desktopresource/upsert.go`
- Test: `/Users/gezhigang/lotus/code/shared/pkg/desktopresource/upsert_test.go`

- [ ] **Step 1: Write helper tests**

Create `/Users/gezhigang/lotus/code/shared/pkg/desktopresource/upsert_test.go`:

```go
package desktopresource

import (
	"testing"

	"lotus/shared/model"
)

func TestValidateResourceRejectsMissingEnglishRuntimeForEmployees(t *testing.T) {
	item := UpsertInput{
		ResourceType: model.DesktopResourceTypeEmployeeTemplate,
		ResourceID:   "builtin:xiaoyuan",
		Version:      "1.0.0",
		Scope:        model.DesktopResourceScopePublic,
		DisplayI18n:  map[string]DisplayText{"zh-CN": {Name: "小研"}, "en-US": {Name: "Researcher"}},
		PromptI18n:   map[string]PromptText{"zh-CN": {Summary: "中文运行提示"}},
	}
	if err := ValidateForPublish(item); err == nil {
		t.Fatal("ValidateForPublish returned nil, want missing en-US prompt error")
	}
}

func TestValidateResourceAcceptsSkillWithoutPromptI18n(t *testing.T) {
	item := UpsertInput{
		ResourceType: model.DesktopResourceTypeSkill,
		ResourceID:   "contract-review",
		Version:      "1.0",
		Scope:        model.DesktopResourceScopePublic,
		DisplayI18n:  map[string]DisplayText{"zh-CN": {Name: "合同审阅"}, "en-US": {Name: "Contract Review"}},
	}
	if err := ValidateForPublish(item); err != nil {
		t.Fatalf("ValidateForPublish returned %v, want nil", err)
	}
}
```

- [ ] **Step 2: Run the failing helper tests**

```bash
cd /Users/gezhigang/lotus/code/shared
go test ./pkg/desktopresource -count=1
```

Expected: FAIL because the package does not exist.

- [ ] **Step 3: Implement validation and upsert input types**

Create `/Users/gezhigang/lotus/code/shared/pkg/desktopresource/upsert.go`:

```go
package desktopresource

import (
	"encoding/json"
	"fmt"
	"time"

	"gorm.io/gorm"

	"lotus/shared/model"
)

type DisplayText struct {
	Name        string   `json:"name"`
	Description string  `json:"description,omitempty"`
	Tagline     string  `json:"tagline,omitempty"`
	Examples    []string `json:"examples,omitempty"`
}

type PromptText struct {
	Summary string `json:"summary"`
}

type UpsertInput struct {
	ResourceType  string
	ResourceID    string
	Version       string
	Scope         string
	TenantID      uint64
	Status        string
	DisplayI18n   map[string]DisplayText
	PromptI18n    map[string]PromptText
	Category      string
	Icon          string
	Featured      bool
	SortOrder     int
	ManifestURL   string
	ManifestSHA256 string
	ManifestSize  int64
	MinDesktopVer string
	CreatedBy     uint64
	PublishedAt   *time.Time
}

func ValidateForPublish(input UpsertInput) error {
	if input.ResourceType == "" || input.ResourceID == "" || input.Version == "" {
		return fmt.Errorf("resource_type, resource_id, and version are required")
	}
	if input.Scope == "" {
		return fmt.Errorf("scope is required")
	}
	for _, lang := range []string{"zh-CN", "en-US"} {
		display, ok := input.DisplayI18n[lang]
		if !ok || display.Name == "" {
			return fmt.Errorf("display_i18n.%s.name is required", lang)
		}
	}
	if input.ResourceType == model.DesktopResourceTypeEmployeeTemplate ||
		input.ResourceType == model.DesktopResourceTypeExpertTeamTemplate {
		for _, lang := range []string{"zh-CN", "en-US"} {
			prompt, ok := input.PromptI18n[lang]
			if !ok || prompt.Summary == "" {
				return fmt.Errorf("prompt_i18n.%s.summary is required", lang)
			}
		}
	}
	return nil
}

func UpsertPublished(db *gorm.DB, input UpsertInput) error {
	if input.Status == "" {
		input.Status = model.DesktopResourceStatusPublished
	}
	if input.PublishedAt == nil {
		now := time.Now()
		input.PublishedAt = &now
	}
	if err := ValidateForPublish(input); err != nil {
		return err
	}
	displayBytes, err := json.Marshal(input.DisplayI18n)
	if err != nil {
		return err
	}
	return db.Where(
		"resource_type = ? AND resource_id = ? AND version = ? AND scope = ? AND tenant_id = ?",
		input.ResourceType, input.ResourceID, input.Version, input.Scope, input.TenantID,
	).Assign(map[string]interface{}{
		"status":              input.Status,
		"display_i18n":        model.JSONRaw(displayBytes),
		"category":            input.Category,
		"icon":                input.Icon,
		"featured":            input.Featured,
		"sort_order":          input.SortOrder,
		"manifest_url":        input.ManifestURL,
		"manifest_sha256":     input.ManifestSHA256,
		"manifest_size":       input.ManifestSize,
		"min_desktop_version": input.MinDesktopVer,
		"created_by":          input.CreatedBy,
		"published_at":        input.PublishedAt,
	}).FirstOrCreate(&model.DesktopResource{
		ResourceType: input.ResourceType,
		ResourceID:   input.ResourceID,
		Version:      input.Version,
		Scope:        input.Scope,
		TenantID:     input.TenantID,
	}).Error
}
```

- [ ] **Step 4: Run helper tests**

```bash
cd /Users/gezhigang/lotus/code/shared
go test ./pkg/desktopresource -count=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /Users/gezhigang/lotus
git add code/shared/pkg/desktopresource/upsert.go code/shared/pkg/desktopresource/upsert_test.go
git commit -m "feat: add desktop resource mirror helper"
```

## Task 3: Gateway `/v1/desktop-resources` Catalog Endpoint

**Files:**
- Create: `/Users/gezhigang/lotus/code/api-gateway/internal/handler/desktop_resources.go`
- Create: `/Users/gezhigang/lotus/code/api-gateway/internal/handler/desktop_resources_test.go`
- Modify: `/Users/gezhigang/lotus/code/api-gateway/cmd/server/main.go`

- [ ] **Step 1: Write gateway handler tests**

Create `/Users/gezhigang/lotus/code/api-gateway/internal/handler/desktop_resources_test.go`:

```go
package handler_test

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"
	"gorm.io/driver/sqlite"
	"gorm.io/gorm"

	"lotus/api-gateway/internal/handler"
	"lotus/shared/model"
)

func TestDesktopResourcesListPublicPublishedOnly(t *testing.T) {
	gin.SetMode(gin.TestMode)
	db, err := gorm.Open(sqlite.Open(":memory:"), &gorm.Config{})
	if err != nil {
		t.Fatal(err)
	}
	if err := db.AutoMigrate(&model.DesktopResource{}); err != nil {
		t.Fatal(err)
	}
	rows := []model.DesktopResource{
		{
			ResourceType: "expert_team_template",
			ResourceID:   "strategy",
			Version:      "1.0.0",
			Scope:        "public",
			TenantID:     0,
			Status:       "published",
			DisplayI18n:  model.JSONRaw(`{"zh-CN":{"name":"战略推演团"},"en-US":{"name":"Strategy Team"}}`),
			ManifestURL:  "https://example.com/strategy.json",
			ManifestSHA256: "abc",
		},
		{
			ResourceType: "expert_team_template",
			ResourceID:   "draft-team",
			Version:      "1.0.0",
			Scope:        "public",
			TenantID:     0,
			Status:       "draft",
			DisplayI18n:  model.JSONRaw(`{"zh-CN":{"name":"草稿"},"en-US":{"name":"Draft"}}`),
		},
		{
			ResourceType: "employee_template",
			ResourceID:   "tenant-only",
			Version:      "1.0.0",
			Scope:        "tenant",
			TenantID:     9,
			Status:       "published",
			DisplayI18n:  model.JSONRaw(`{"zh-CN":{"name":"租户"},"en-US":{"name":"Tenant"}}`),
		},
	}
	if err := db.Create(&rows).Error; err != nil {
		t.Fatal(err)
	}

	r := gin.New()
	h := &handler.DesktopResourcesHandler{DB: db}
	r.GET("/v1/desktop-resources", func(c *gin.Context) {
		c.Set("tenant_id", uint64(1))
		h.List(c)
	})

	req := httptest.NewRequest(http.MethodGet, "/v1/desktop-resources?lang=en-US", nil)
	w := httptest.NewRecorder()
	r.ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Fatalf("status = %d body=%s", w.Code, w.Body.String())
	}
	body := w.Body.String()
	if !strings.Contains(body, `"resourceId":"strategy"`) {
		t.Fatalf("expected strategy in body: %s", body)
	}
	if !strings.Contains(body, `"name":"Strategy Team"`) {
		t.Fatalf("expected English projection in body: %s", body)
	}
	if strings.Contains(body, "draft-team") || strings.Contains(body, "tenant-only") {
		t.Fatalf("unexpected non-public/non-published resource in body: %s", body)
	}
}
```

- [ ] **Step 2: Run the failing gateway test**

```bash
cd /Users/gezhigang/lotus/code/api-gateway
go test ./internal/handler -run TestDesktopResources -count=1
```

Expected: FAIL because `DesktopResourcesHandler` is undefined.

- [ ] **Step 3: Implement the handler**

Create `/Users/gezhigang/lotus/code/api-gateway/internal/handler/desktop_resources.go`:

```go
package handler

import (
	"encoding/json"
	"net/http"
	"strings"

	"github.com/gin-gonic/gin"
	"gorm.io/gorm"

	"lotus/shared/model"
)

type DesktopResourcesHandler struct {
	DB *gorm.DB
}

type desktopResourceDisplay struct {
	Name        string   `json:"name"`
	Description string  `json:"description,omitempty"`
	Tagline     string  `json:"tagline,omitempty"`
	Examples    []string `json:"examples,omitempty"`
}

type desktopResourceResponse struct {
	ResourceType     string                 `json:"resourceType"`
	ResourceID       string                 `json:"resourceId"`
	Version          string                 `json:"version"`
	Scope            string                 `json:"scope"`
	Display          desktopResourceDisplay `json:"display"`
	Category         string                 `json:"category,omitempty"`
	Icon             string                 `json:"icon,omitempty"`
	Featured         bool                   `json:"featured"`
	ManifestURL      string                 `json:"manifestUrl"`
	ManifestSHA256   string                 `json:"manifestSha256"`
	ManifestSize     int64                  `json:"manifestSize"`
	MinDesktopVersion string                `json:"minDesktopVersion,omitempty"`
}

func (h *DesktopResourcesHandler) List(c *gin.Context) {
	lang := c.DefaultQuery("lang", "zh-CN")
	if lang != "en-US" && lang != "zh-CN" {
		lang = "zh-CN"
	}
	types := parseDesktopResourceTypes(c.Query("types"))

	query := h.DB.Model(&model.DesktopResource{}).
		Where("scope = ? AND tenant_id = ? AND status = ?",
			model.DesktopResourceScopePublic,
			0,
			model.DesktopResourceStatusPublished,
		)
	if len(types) > 0 {
		query = query.Where("resource_type IN ?", types)
	}

	var rows []model.DesktopResource
	if err := query.Order("featured DESC, sort_order ASC, updated_at DESC").Find(&rows).Error; err != nil {
		respondGatewayError(c, http.StatusInternalServerError, "internal_error", "Failed to list desktop resources")
		return
	}

	out := make([]desktopResourceResponse, 0, len(rows))
	for _, row := range rows {
		out = append(out, desktopResourceResponse{
			ResourceType:      row.ResourceType,
			ResourceID:        row.ResourceID,
			Version:           row.Version,
			Scope:             row.Scope,
			Display:           selectDesktopResourceDisplay(row.DisplayI18n, lang),
			Category:          row.Category,
			Icon:              row.Icon,
			Featured:          row.Featured,
			ManifestURL:       row.ManifestURL,
			ManifestSHA256:    row.ManifestSHA256,
			ManifestSize:      row.ManifestSize,
			MinDesktopVersion: row.MinDesktopVer,
		})
	}
	c.JSON(http.StatusOK, gin.H{"data": out})
}

func parseDesktopResourceTypes(raw string) []string {
	if strings.TrimSpace(raw) == "" {
		return nil
	}
	allowed := map[string]bool{
		model.DesktopResourceTypeSkill: true,
		model.DesktopResourceTypeEmployeeTemplate: true,
		model.DesktopResourceTypeExpertTeamTemplate: true,
	}
	parts := strings.Split(raw, ",")
	out := make([]string, 0, len(parts))
	for _, part := range parts {
		v := strings.TrimSpace(part)
		if allowed[v] {
			out = append(out, v)
		}
	}
	return out
}

func selectDesktopResourceDisplay(raw model.JSONRaw, lang string) desktopResourceDisplay {
	var byLang map[string]desktopResourceDisplay
	if len(raw) > 0 {
		_ = json.Unmarshal(raw, &byLang)
	}
	if display, ok := byLang[lang]; ok && display.Name != "" {
		return display
	}
	if display, ok := byLang["zh-CN"]; ok {
		return display
	}
	return desktopResourceDisplay{}
}
```

- [ ] **Step 4: Register the route**

In `/Users/gezhigang/lotus/code/api-gateway/cmd/server/main.go`:

1. Add after `modelsHandler := ...`:

```go
desktopResourcesHandler := &handler.DesktopResourcesHandler{DB: db}
```

2. Inside the authenticated `/v1` group, add:

```go
v1.GET("/desktop-resources", desktopResourcesHandler.List)
```

- [ ] **Step 5: Run gateway handler tests**

```bash
cd /Users/gezhigang/lotus/code/api-gateway
go test ./internal/handler -run TestDesktopResources -count=1
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cd /Users/gezhigang/lotus
git add code/api-gateway/internal/handler/desktop_resources.go code/api-gateway/internal/handler/desktop_resources_test.go code/api-gateway/cmd/server/main.go
git commit -m "feat: expose desktop resource catalog"
```

## Task 4: Mirror Public Skill Publishes Into `desktop_resources`

**Files:**
- Modify: `/Users/gezhigang/lotus/code/ops-portal/server/internal/handler/skill_marketplace.go`
- Test: `/Users/gezhigang/lotus/code/ops-portal/server/internal/handler/skill_marketplace_test.go`

- [ ] **Step 1: Add a publish mirror test**

If `skill_marketplace_test.go` does not exist, create it. Add this focused test around the helper used by `Publish`:

```go
package handler

import (
	"testing"

	"gorm.io/driver/sqlite"
	"gorm.io/gorm"

	"lotus/shared/model"
)

func TestMirrorSkillPackageToDesktopResource(t *testing.T) {
	db, err := gorm.Open(sqlite.Open(":memory:"), &gorm.Config{})
	if err != nil {
		t.Fatal(err)
	}
	if err := db.AutoMigrate(&model.SkillPackage{}, &model.DesktopResource{}); err != nil {
		t.Fatal(err)
	}
	pkg := model.SkillPackage{
		TenantID:    0,
		PluginID:    "contract-review",
		Name:        "合同审阅",
		Description: "审阅合同风险",
		Category:    "legal",
		Icon:        "⚖️",
		Version:     "1.0",
		Scope:       "public",
		Status:      "published",
		PackageURL:  "https://example.com/contract.zip",
		Sha256:      "abc",
		PackageSize: 100,
	}
	if err := mirrorSkillPackageToDesktopResource(db, pkg); err != nil {
		t.Fatal(err)
	}
	var row model.DesktopResource
	if err := db.First(&row, "resource_type = ? AND resource_id = ?", model.DesktopResourceTypeSkill, "contract-review").Error; err != nil {
		t.Fatal(err)
	}
	if row.ManifestURL != pkg.PackageURL || row.ManifestSHA256 != pkg.Sha256 {
		t.Fatalf("mirror row mismatch: %#v", row)
	}
}
```

- [ ] **Step 2: Run the failing OPS handler test**

```bash
cd /Users/gezhigang/lotus/code/ops-portal/server
go test ./internal/handler -run TestMirrorSkillPackageToDesktopResource -count=1
```

Expected: FAIL because `mirrorSkillPackageToDesktopResource` is undefined.

- [ ] **Step 3: Add the mirror helper and call it after publish**

In `/Users/gezhigang/lotus/code/ops-portal/server/internal/handler/skill_marketplace.go`, add imports:

```go
	"lotus/shared/pkg/desktopresource"
```

Add helper near the bottom:

```go
func mirrorSkillPackageToDesktopResource(db *gorm.DB, pkg model.SkillPackage) error {
	if pkg.Scope != "public" || pkg.Status != "published" {
		return nil
	}
	return desktopresource.UpsertPublished(db, desktopresource.UpsertInput{
		ResourceType: model.DesktopResourceTypeSkill,
		ResourceID:   pkg.PluginID,
		Version:      pkg.Version,
		Scope:        model.DesktopResourceScopePublic,
		TenantID:     0,
		DisplayI18n: map[string]desktopresource.DisplayText{
			"zh-CN": {Name: pkg.Name, Description: pkg.Description},
			"en-US": {Name: pkg.Name, Description: pkg.Description},
		},
		Category:     pkg.Category,
		Icon:         pkg.Icon,
		Featured:     pkg.Featured,
		ManifestURL:  pkg.PackageURL,
		ManifestSHA256: pkg.Sha256,
		ManifestSize: pkg.PackageSize,
		CreatedBy:    pkg.CreatedBy,
	})
}
```

After the publish path creates/reloads `pkg`, call:

```go
if err := mirrorSkillPackageToDesktopResource(h.DB, pkg); err != nil {
	h.Log.Warn("failed to mirror skill package to desktop_resources",
		zap.Uint64("skill_package_id", pkg.ID),
		zap.Error(err))
}
```

Also call this helper after `UpdateMeta` and `Feature` when the row remains `scope=public,status=published`.

- [ ] **Step 4: Run the OPS handler test**

```bash
cd /Users/gezhigang/lotus/code/ops-portal/server
go test ./internal/handler -run TestMirrorSkillPackageToDesktopResource -count=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /Users/gezhigang/lotus
git add code/ops-portal/server/internal/handler/skill_marketplace.go code/ops-portal/server/internal/handler/skill_marketplace_test.go
git commit -m "feat: mirror public skills to desktop resources"
```

## Task 5: Employee Template Bilingual Snapshot and Catalog Mirror

**Files:**
- Modify: `/Users/gezhigang/lotus/code/shared/model/employee_template.go`
- Modify: `/Users/gezhigang/lotus/code/ops-portal/server/internal/handler/employee_template.go`
- Test: `/Users/gezhigang/lotus/code/ops-portal/server/internal/handler/employee_template_test.go`

- [ ] **Step 1: Add employee publish validation tests**

Append to `/Users/gezhigang/lotus/code/ops-portal/server/internal/handler/employee_template_test.go`:

```go
func TestValidateEmployeeTemplateI18nForPublish(t *testing.T) {
	tpl := model.EmployeeTemplate{
		TemplateID: "builtin:xiaoyuan",
		Version:    "1.0.0",
		Name:       "小研",
		DisplayI18n: model.JSONRaw(`{"zh-CN":{"name":"小研"},"en-US":{"name":"Researcher"}}`),
		PromptI18n:  model.JSONRaw(`{"zh-CN":{"systemPromptExtra":"中文"},"en-US":{"systemPromptExtra":"English"}}`),
	}
	if err := validateEmployeeTemplateI18nForPublish(&tpl); err != nil {
		t.Fatalf("expected valid i18n, got %v", err)
	}

	tpl.PromptI18n = model.JSONRaw(`{"zh-CN":{"systemPromptExtra":"中文"}}`)
	if err := validateEmployeeTemplateI18nForPublish(&tpl); err == nil {
		t.Fatal("expected missing en-US prompt validation error")
	}
}
```

- [ ] **Step 2: Run the failing employee test**

```bash
cd /Users/gezhigang/lotus/code/ops-portal/server
go test ./internal/handler -run TestValidateEmployeeTemplateI18nForPublish -count=1
```

Expected: FAIL because `DisplayI18n`, `PromptI18n`, and validation helper are undefined.

- [ ] **Step 3: Extend `EmployeeTemplate` model**

In `/Users/gezhigang/lotus/code/shared/model/employee_template.go`, add fields after `Badge`:

```go
	DisplayI18n JSONRaw `gorm:"type:text" json:"display_i18n"`
	PromptI18n  JSONRaw `gorm:"type:text" json:"prompt_i18n"`
	SchemaI18n  JSONRaw `gorm:"type:text" json:"schema_i18n"`
```

- [ ] **Step 4: Add validation and snapshot fields**

In `/Users/gezhigang/lotus/code/ops-portal/server/internal/handler/employee_template.go`, add validation:

```go
func validateEmployeeTemplateI18nForPublish(tpl *model.EmployeeTemplate) error {
	var display map[string]map[string]string
	if len(tpl.DisplayI18n) == 0 || json.Unmarshal(tpl.DisplayI18n, &display) != nil {
		return fmt.Errorf("display_i18n must be valid JSON")
	}
	var prompt map[string]map[string]string
	if len(tpl.PromptI18n) == 0 || json.Unmarshal(tpl.PromptI18n, &prompt) != nil {
		return fmt.Errorf("prompt_i18n must be valid JSON")
	}
	for _, lang := range []string{"zh-CN", "en-US"} {
		if display[lang]["name"] == "" {
			return fmt.Errorf("display_i18n.%s.name is required", lang)
		}
		if prompt[lang]["systemPromptExtra"] == "" {
			return fmt.Errorf("prompt_i18n.%s.systemPromptExtra is required", lang)
		}
	}
	return nil
}
```

Call it at the top of `Publish` before `json.MarshalIndent(snapshotForDesktop(&tpl), "", "  ")`:

```go
if err := validateEmployeeTemplateI18nForPublish(&tpl); err != nil {
	response.BadRequest(c, err.Error())
	return
}
```

Extend `desktopTemplateSnapshot`:

```go
	DisplayI18n json.RawMessage `json:"displayI18n,omitempty"`
	PromptI18n  json.RawMessage `json:"promptI18n,omitempty"`
	SchemaI18n  json.RawMessage `json:"schemaI18n,omitempty"`
```

Set the fields in `snapshotForDesktop`:

```go
		DisplayI18n: []byte(tpl.DisplayI18n),
		PromptI18n:  []byte(tpl.PromptI18n),
		SchemaI18n:  []byte(tpl.SchemaI18n),
```

- [ ] **Step 5: Mirror employee template publish into `desktop_resources`**

Import:

```go
	"lotus/shared/pkg/desktopresource"
```

Add helper:

```go
func mirrorEmployeeTemplateToDesktopResource(db *gorm.DB, tpl model.EmployeeTemplate) error {
	if tpl.Status != "published" || tpl.TenantScope != "global" {
		return nil
	}
	var display map[string]desktopresource.DisplayText
	if err := json.Unmarshal(tpl.DisplayI18n, &display); err != nil {
		return err
	}
	var promptRaw map[string]map[string]string
	if err := json.Unmarshal(tpl.PromptI18n, &promptRaw); err != nil {
		return err
	}
	prompt := map[string]desktopresource.PromptText{}
	for lang, v := range promptRaw {
		prompt[lang] = desktopresource.PromptText{Summary: v["systemPromptExtra"]}
	}
	return desktopresource.UpsertPublished(db, desktopresource.UpsertInput{
		ResourceType: model.DesktopResourceTypeEmployeeTemplate,
		ResourceID:   tpl.TemplateID,
		Version:      tpl.Version,
		Scope:        model.DesktopResourceScopePublic,
		TenantID:     0,
		DisplayI18n:  display,
		PromptI18n:   prompt,
		Category:     "employee",
		Icon:         tpl.Avatar,
		ManifestURL:  tpl.PackageURL,
		ManifestSHA256: tpl.PackageSHA,
		ManifestSize: tpl.PackageSize,
		CreatedBy:    tpl.CreatedBy,
		PublishedAt:  tpl.PublishedAt,
	})
}
```

After `Publish` reloads `tpl`, call:

```go
if err := mirrorEmployeeTemplateToDesktopResource(h.DB, tpl); err != nil {
	h.Log.Warn("failed to mirror employee template to desktop_resources",
		zap.Uint64("employee_template_id", tpl.ID),
		zap.Error(err))
}
```

- [ ] **Step 6: Run employee handler tests**

```bash
cd /Users/gezhigang/lotus/code/ops-portal/server
go test ./internal/handler -run 'TestValidateEmployeeTemplateI18nForPublish|TestMirrorEmployee' -count=1
```

Expected: PASS for validation tests. If no mirror-specific test exists yet, add one equivalent to Task 4 using `mirrorEmployeeTemplateToDesktopResource`.

- [ ] **Step 7: Commit**

```bash
cd /Users/gezhigang/lotus
git add code/shared/model/employee_template.go code/ops-portal/server/internal/handler/employee_template.go code/ops-portal/server/internal/handler/employee_template_test.go
git commit -m "feat: add bilingual employee template catalog metadata"
```

## Task 6: Expert-Team Template Model and OPS Publish API

**Files:**
- Create: `/Users/gezhigang/lotus/code/shared/model/expert_team_template.go`
- Modify: `/Users/gezhigang/lotus/code/shared/migration/migrate.go`
- Create: `/Users/gezhigang/lotus/code/ops-portal/server/internal/handler/expert_team_template.go`
- Create: `/Users/gezhigang/lotus/code/ops-portal/server/internal/handler/expert_team_template_test.go`
- Modify: `/Users/gezhigang/lotus/code/ops-portal/server/cmd/server/main.go`

- [ ] **Step 1: Write expert stable-name validation tests**

Create `/Users/gezhigang/lotus/code/ops-portal/server/internal/handler/expert_team_template_test.go`:

```go
package handler

import "testing"

func TestValidateExpertStableName(t *testing.T) {
	valid := []string{"cfo", "strategy-advisor", "growth-hacker"}
	for _, name := range valid {
		if err := validateExpertStableName(name); err != nil {
			t.Fatalf("validateExpertStableName(%q) = %v, want nil", name, err)
		}
	}
	invalid := []string{"CFO", "财务", "-bad", "bad_", "a"}
	for _, name := range invalid {
		if err := validateExpertStableName(name); err == nil {
			t.Fatalf("validateExpertStableName(%q) returned nil, want error", name)
		}
	}
}
```

- [ ] **Step 2: Run the failing expert-team test**

```bash
cd /Users/gezhigang/lotus/code/ops-portal/server
go test ./internal/handler -run TestValidateExpertStableName -count=1
```

Expected: FAIL because `validateExpertStableName` is undefined.

- [ ] **Step 3: Add shared model**

Create `/Users/gezhigang/lotus/code/shared/model/expert_team_template.go`:

```go
package model

import "time"

// ExpertTeamTemplate is a versioned public/tenant expert-team snapshot.
type ExpertTeamTemplate struct {
	ID                uint64  `gorm:"primaryKey;autoIncrement" json:"id"`
	TeamID            string  `gorm:"type:varchar(64);not null;uniqueIndex:uk_expert_team_version,priority:1;index:idx_expert_team_status" json:"team_id"`
	Version           string  `gorm:"type:varchar(32);not null;uniqueIndex:uk_expert_team_version,priority:2" json:"version"`
	TenantScope       string  `gorm:"type:varchar(64);default:'global';index:idx_expert_team_scope" json:"tenant_scope"`
	Status            string  `gorm:"type:varchar(20);default:'draft';index:idx_expert_team_status" json:"status"`
	FacilitationStyle string  `gorm:"type:varchar(20);not null" json:"facilitation_style"`
	DisplayI18n       JSONRaw `gorm:"type:text" json:"display_i18n"`
	Experts           JSONRaw `gorm:"type:text" json:"experts"`
	DirectorPromptI18n JSONRaw `gorm:"type:text" json:"director_prompt_i18n"`
	PackageURL        string  `gorm:"type:varchar(500)" json:"package_url,omitempty"`
	PackageSHA        string  `gorm:"type:varchar(64)" json:"package_sha256,omitempty"`
	PackageSize       int64   `gorm:"default:0" json:"package_size,omitempty"`
	PublishedAt       *time.Time `json:"published_at,omitempty"`
	CreatedBy         uint64  `gorm:"not null" json:"created_by"`
	CreatedAt         time.Time `gorm:"autoCreateTime" json:"created_at"`
	UpdatedAt         time.Time `gorm:"autoUpdateTime" json:"updated_at"`
}

func (ExpertTeamTemplate) TableName() string { return "expert_team_templates" }
```

Add `&model.ExpertTeamTemplate{},` to `runMigration` model list after `&model.DesktopResource{},`.

- [ ] **Step 4: Implement handler validation and snapshot shaping**

Create `/Users/gezhigang/lotus/code/ops-portal/server/internal/handler/expert_team_template.go` with these validation helpers first:

```go
package handler

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"regexp"
)

var expertStableNamePattern = regexp.MustCompile(`^[a-z][a-z0-9-]{1,63}$`)

func validateExpertStableName(name string) error {
	if !expertStableNamePattern.MatchString(name) {
		return fmt.Errorf("stable_name must match ^[a-z][a-z0-9-]{1,63}$")
	}
	return nil
}

type expertTeamSnapshot struct {
	TeamID             string          `json:"teamId"`
	Version            string          `json:"version"`
	FacilitationStyle  string          `json:"facilitationStyle"`
	DisplayI18n        json.RawMessage `json:"displayI18n"`
	Experts            json.RawMessage `json:"experts"`
	DirectorPromptI18n json.RawMessage `json:"directorPromptI18n"`
}

func expertTeamSnapshotBytes(teamID, version, style string, display, experts, prompts []byte) ([]byte, string, error) {
	snap := expertTeamSnapshot{
		TeamID: teamID,
		Version: version,
		FacilitationStyle: style,
		DisplayI18n: display,
		Experts: experts,
		DirectorPromptI18n: prompts,
	}
	data, err := json.MarshalIndent(snap, "", "  ")
	if err != nil {
		return nil, "", err
	}
	sum := sha256.Sum256(data)
	return data, hex.EncodeToString(sum[:]), nil
}
```

Then implement CRUD/publish by mirroring `EmployeeTemplateHandler` structure:

- `List`
- `GetByID`
- `Create`
- `Update` for draft rows only
- `Publish`
- `Deprecate`
- `Delete`

During `Publish`, validate:

```go
if tpl.TenantScope != "global" {
	response.BadRequest(c, "phase 1 only publishes global expert teams from OPS")
	return
}
if tpl.FacilitationStyle != "rounds" && tpl.FacilitationStyle != "debate" && tpl.FacilitationStyle != "open" {
	response.BadRequest(c, "facilitation_style must be rounds, debate, or open")
	return
}
```

Parse `Experts` and validate each `stableName`:

```go
var experts []struct {
	StableName string `json:"stableName"`
}
if err := json.Unmarshal(tpl.Experts, &experts); err != nil {
	response.BadRequest(c, "experts must be valid JSON")
	return
}
for _, expert := range experts {
	if err := validateExpertStableName(expert.StableName); err != nil {
		response.BadRequest(c, err.Error())
		return
	}
}
```

Upload canonical JSON to OSS path:

```go
objectKey := fmt.Sprintf("ops/expert-teams/%s/%s.json", tpl.TeamID, tpl.Version)
```

After upload and DB update, mirror to `desktop_resources` with:

```go
desktopresource.UpsertPublished(h.DB, desktopresource.UpsertInput{
	ResourceType: model.DesktopResourceTypeExpertTeamTemplate,
	ResourceID: tpl.TeamID,
	Version: tpl.Version,
	Scope: model.DesktopResourceScopePublic,
	TenantID: 0,
	DisplayI18n: display,
	PromptI18n: prompt,
	Category: "expert-team",
	Icon: "users",
	ManifestURL: tpl.PackageURL,
	ManifestSHA256: tpl.PackageSHA,
	ManifestSize: tpl.PackageSize,
	CreatedBy: tpl.CreatedBy,
	PublishedAt: tpl.PublishedAt,
})
```

- [ ] **Step 5: Register OPS routes**

In `/Users/gezhigang/lotus/code/ops-portal/server/cmd/server/main.go`, instantiate:

```go
expertTeamTemplateHandler := &handler.ExpertTeamTemplateHandler{DB: db, Cfg: &cfg, Log: log}
```

Register under the existing authenticated API group:

```go
expertTeams := api.Group("/expert-team-templates")
{
	expertTeams.GET("", expertTeamTemplateHandler.List)
	expertTeams.POST("", expertTeamTemplateHandler.Create)
	expertTeams.GET("/:id", expertTeamTemplateHandler.GetByID)
	expertTeams.PUT("/:id", expertTeamTemplateHandler.Update)
	expertTeams.PUT("/:id/publish", expertTeamTemplateHandler.Publish)
	expertTeams.PUT("/:id/deprecate", expertTeamTemplateHandler.Deprecate)
	expertTeams.DELETE("/:id", expertTeamTemplateHandler.Delete)
}
```

Use the actual API group variable name in `main.go`; keep route shape exact.

- [ ] **Step 6: Run expert-team handler tests and server compile**

```bash
cd /Users/gezhigang/lotus/code/ops-portal/server
go test ./internal/handler -run TestValidateExpertStableName -count=1
go test ./cmd/server -run TestDoesNotExist -count=1
```

Expected:

- First command: PASS.
- Second command may report `[no test files]`; it must compile.

- [ ] **Step 7: Commit**

```bash
cd /Users/gezhigang/lotus
git add code/shared/model/expert_team_template.go code/shared/migration/migrate.go code/ops-portal/server/internal/handler/expert_team_template.go code/ops-portal/server/internal/handler/expert_team_template_test.go code/ops-portal/server/cmd/server/main.go
git commit -m "feat: add expert team template publishing"
```

## Task 7: Desktop Resource Catalog Rust Module

**Files:**
- Create: `src-tauri/src/runtime/desktop_resources/mod.rs`
- Create: `src-tauri/src/runtime/desktop_resources/catalog.rs`
- Create: `src-tauri/src/runtime/desktop_resources/sync.rs`
- Modify: `src-tauri/src/runtime/mod.rs`
- Create: `src-tauri/src/commands/desktop_resources.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write Rust catalog tests**

Create `src-tauri/src/runtime/desktop_resources/catalog.rs` with tests first:

```rust
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopResourceDisplay {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tagline: String,
    #[serde(default)]
    pub examples: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopResourceItem {
    pub resource_type: String,
    pub resource_id: String,
    pub version: String,
    pub scope: String,
    pub display: DesktopResourceDisplay,
    #[serde(default)]
    pub manifest_url: String,
    #[serde(default)]
    pub manifest_sha256: String,
    #[serde(default)]
    pub manifest_size: i64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopResourceIndex {
    #[serde(default)]
    pub resources: BTreeMap<String, DesktopResourceItem>,
}

pub fn resource_key(item: &DesktopResourceItem) -> String {
    format!("{}:{}:{}", item.resource_type, item.resource_id, item.scope)
}

fn parse_version_part(part: &str) -> u64 {
    let digits = part
        .split(|ch: char| !ch.is_ascii_digit())
        .next()
        .unwrap_or("");
    digits.parse::<u64>().unwrap_or(0)
}

pub fn compare_versions(left: &str, right: &str) -> Ordering {
    let left_parts: Vec<&str> = left.split('.').collect();
    let right_parts: Vec<&str> = right.split('.').collect();
    let len = left_parts.len().max(right_parts.len());
    for idx in 0..len {
        let l = left_parts.get(idx).map(|part| parse_version_part(part)).unwrap_or(0);
        let r = right_parts.get(idx).map(|part| parse_version_part(part)).unwrap_or(0);
        match l.cmp(&r) {
            Ordering::Equal => continue,
            ordering => return ordering,
        }
    }
    Ordering::Equal
}

pub fn select_newer<'a>(
    current: Option<&'a DesktopResourceItem>,
    incoming: &'a DesktopResourceItem,
) -> &'a DesktopResourceItem {
    match current {
        Some(existing) if compare_versions(&existing.version, &incoming.version) != Ordering::Less => existing,
        _ => incoming,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_key_includes_type_id_and_scope() {
        let item = DesktopResourceItem {
            resource_type: "expert_team_template".into(),
            resource_id: "strategy".into(),
            version: "1.0.0".into(),
            scope: "public".into(),
            display: DesktopResourceDisplay { name: "Strategy".into(), description: String::new(), tagline: String::new(), examples: vec![] },
            manifest_url: String::new(),
            manifest_sha256: String::new(),
            manifest_size: 0,
        };
        assert_eq!(resource_key(&item), "expert_team_template:strategy:public");
    }

    #[test]
    fn select_newer_uses_numeric_version_segments() {
        let older = DesktopResourceItem {
            resource_type: "employee_template".into(),
            resource_id: "builtin:xiaoyuan".into(),
            version: "1.0.0".into(),
            scope: "public".into(),
            display: DesktopResourceDisplay { name: "A".into(), description: String::new(), tagline: String::new(), examples: vec![] },
            manifest_url: String::new(),
            manifest_sha256: String::new(),
            manifest_size: 0,
        };
        let newer = DesktopResourceItem { version: "1.10.0".into(), ..older.clone() };
        let middle = DesktopResourceItem { version: "1.2.0".into(), ..older.clone() };
        assert_eq!(select_newer(Some(&middle), &newer).version, "1.10.0");
        assert_eq!(select_newer(Some(&newer), &middle).version, "1.10.0");
    }
}
```

- [ ] **Step 2: Wire module and run tests**

Create `src-tauri/src/runtime/desktop_resources/mod.rs`:

```rust
pub mod catalog;
pub mod sync;
```

Add to `src-tauri/src/runtime/mod.rs`:

```rust
pub mod desktop_resources;
```

Create a minimal `src-tauri/src/runtime/desktop_resources/sync.rs`:

```rust
use super::catalog::{compare_versions, resource_key, DesktopResourceIndex, DesktopResourceItem};
use std::cmp::Ordering;

pub fn merge_catalog_items(items: Vec<DesktopResourceItem>) -> DesktopResourceIndex {
    let mut index = DesktopResourceIndex::default();
    for item in items {
        let key = resource_key(&item);
        let replace = index
            .resources
            .get(&key)
            .map(|existing| compare_versions(&existing.version, &item.version) == Ordering::Less)
            .unwrap_or(true);
        if replace {
            index.resources.insert(key, item);
        }
    }
    index
}
```

Run:

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
cargo test desktop_resources::catalog --lib
```

Expected: PASS.

- [ ] **Step 3: Add basic Tauri commands**

Create `src-tauri/src/commands/desktop_resources.rs`:

```rust
use crate::runtime::desktop_resources::catalog::DesktopResourceIndex;

#[tauri::command]
pub async fn sync_desktop_resources() -> Result<DesktopResourceIndex, String> {
    Ok(DesktopResourceIndex::default())
}

#[tauri::command]
pub async fn get_desktop_resource_status() -> Result<DesktopResourceIndex, String> {
    Ok(DesktopResourceIndex::default())
}
```

Add to `src-tauri/src/commands/mod.rs`:

```rust
pub mod desktop_resources;
```

Register in `src-tauri/src/lib.rs` `generate_handler!`:

```rust
commands::desktop_resources::sync_desktop_resources,
commands::desktop_resources::get_desktop_resource_status,
```

- [ ] **Step 4: Compile Tauri command surface**

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
cargo check
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /Users/gezhigang/work-codeup/aijia/code
git add src-tauri/src/runtime/desktop_resources src-tauri/src/runtime/mod.rs src-tauri/src/commands/desktop_resources.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat: add desktop resource sync shell"
```

## Task 8: Desktop Expert-Team Snapshot Store and Bootstrap

**Files:**
- Create: `src-tauri/src/runtime/expert_team/mod.rs`
- Create: `src-tauri/src/runtime/expert_team/store.rs`
- Create: `src-tauri/src/runtime/expert_team/expert_teams_bootstrap.json`
- Modify: `src-tauri/src/runtime/mod.rs`
- Modify: `src-tauri/src/storage/aijia_home.rs`
- Create: `src-tauri/src/commands/expert_teams.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write Rust expert-team store tests**

Create `src-tauri/src/runtime/expert_team/store.rs`:

```rust
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalizedText {
    pub name: String,
    #[serde(default)]
    pub tagline: String,
    #[serde(default)]
    pub examples: Vec<String>,
    #[serde(default)]
    pub composer_placeholder: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExpertPersonaSnapshot {
    pub stable_name: String,
    #[serde(default)]
    pub emoji: String,
    #[serde(default)]
    pub display_i18n: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    pub prompt_i18n: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExpertTeamSnapshot {
    pub team_id: String,
    pub version: String,
    pub facilitation_style: String,
    pub display_i18n: BTreeMap<String, LocalizedText>,
    #[serde(default)]
    pub experts: Vec<ExpertPersonaSnapshot>,
    #[serde(default)]
    pub director_prompt_i18n: BTreeMap<String, BTreeMap<String, String>>,
}

const BOOTSTRAP_JSON: &str = include_str!("expert_teams_bootstrap.json");

pub fn bootstrap_teams() -> Result<Vec<ExpertTeamSnapshot>> {
    serde_json::from_str(BOOTSTRAP_JSON).context("parse expert team bootstrap JSON")
}

pub fn cache_path(cache_dir: &Path, team_id: &str, version: &str) -> PathBuf {
    cache_dir.join(team_id).join(format!("{version}.json"))
}

pub fn write_cache(cache_dir: &Path, snapshot: &ExpertTeamSnapshot) -> Result<PathBuf> {
    let dir = cache_dir.join(&snapshot.team_id);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", snapshot.version));
    fs::write(&path, serde_json::to_vec_pretty(snapshot)?)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_teams_parse() {
        let teams = bootstrap_teams().expect("bootstrap parses");
        assert!(teams.iter().any(|team| team.team_id == "strategy"));
        for team in teams {
            assert!(team.display_i18n.contains_key("zh-CN"));
            assert!(team.display_i18n.contains_key("en-US"));
        }
    }

    #[test]
    fn cache_path_is_team_and_version_scoped() {
        let path = cache_path(Path::new("/tmp/cache"), "strategy", "1.0.0");
        assert_eq!(path, Path::new("/tmp/cache/strategy/1.0.0.json"));
    }
}
```

- [ ] **Step 2: Add bootstrap data**

Create `src-tauri/src/runtime/expert_team/expert_teams_bootstrap.json` with at least the strategy and roundtable teams first:

```json
[
  {
    "teamId": "strategy",
    "version": "1.0.0",
    "facilitationStyle": "rounds",
    "displayI18n": {
      "zh-CN": {
        "name": "战略推演团",
        "tagline": "重大决策前的多视角压力测试",
        "examples": ["是否拓展东南亚市场", "是否启动 B 轮融资"],
        "composerPlaceholder": "告诉他们你想推演什么决策..."
      },
      "en-US": {
        "name": "Strategy Simulation Team",
        "tagline": "Stress-test major decisions from multiple angles",
        "examples": ["Should we expand into Southeast Asia?", "Should we start Series B fundraising?"],
        "composerPlaceholder": "Tell the team what decision you want to test..."
      }
    },
    "experts": [
      {
        "stableName": "strategy-advisor",
        "emoji": "🧠",
        "displayI18n": {
          "zh-CN": { "name": "战略顾问" },
          "en-US": { "name": "Strategy Advisor" }
        },
        "promptI18n": {
          "zh-CN": { "persona": "麦肯锡式严谨，擅长 SWOT / 五力分析" },
          "en-US": { "persona": "Structured strategy advisor, strong at SWOT and five-forces analysis" }
        }
      },
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
      "zh-CN": { "template": "你现在的任务是为用户主持一场「{teamName}」圆桌讨论。\n\n# 团队成员\n{roster}\n\n# 用户提出的议题\n{topic}\n\n# 执行要求\n1. 调用 TeamCreate 创建团队（team_name = \"{teamName}\"）\n2. 为每位专家分别用 Agent 工具 spawn 子代理\n3. spawn 子代理时 name 参数必须使用专家 stableName\n4. 让每位专家发表观点并互相点评\n5. 你作为主持人整理最终建议" },
      "en-US": { "template": "Your task is to facilitate a {teamName} roundtable.\n\n# Team Members\n{roster}\n\n# User Topic\n{topic}\n\n# Requirements\n1. Call TeamCreate with team_name = \"{teamName}\"\n2. Spawn one Agent subagent for each expert\n3. Use each expert stableName as the Agent name parameter\n4. Ask each expert to present and critique views\n5. As lead facilitator, summarize final recommendations" }
    }
  },
  {
    "teamId": "roundtable",
    "version": "1.0.0",
    "facilitationStyle": "open",
    "displayI18n": {
      "zh-CN": {
        "name": "圆桌讨论团",
        "tagline": "开放议题 / 不确定角色构成",
        "examples": ["团队五年后的工作形态会是怎样"],
        "composerPlaceholder": "抛出你的议题，主持人会召集合适的专家..."
      },
      "en-US": {
        "name": "Open Roundtable",
        "tagline": "Open topics with adaptive expert roles",
        "examples": ["What will our team's work look like in five years?"],
        "composerPlaceholder": "Share a topic and the facilitator will assemble the right experts..."
      }
    },
    "experts": [],
    "directorPromptI18n": {
      "zh-CN": { "template": "你现在的任务是为用户主持一场「{teamName}」开放圆桌讨论。\n\n# 用户提出的议题\n{topic}\n\n# 执行要求\n1. 先判断议题需要哪些专业视角\n2. 调用 TeamCreate 创建团队（team_name = \"{teamName}\"）\n3. 为 3-5 位合适专家 spawn 子代理，使用稳定英文 stableName\n4. 汇总观点并给出结论" },
      "en-US": { "template": "Your task is to facilitate an open {teamName} discussion.\n\n# User Topic\n{topic}\n\n# Requirements\n1. Decide which expert perspectives are needed\n2. Call TeamCreate with team_name = \"{teamName}\"\n3. Spawn 3-5 suitable expert subagents with stable English stableName values\n4. Synthesize the views and provide a conclusion" }
    }
  }
]
```

After the first pass is green, convert the remaining six existing hardcoded teams into this file in the same shape.

- [ ] **Step 3: Wire module and cache path**

Create `src-tauri/src/runtime/expert_team/mod.rs`:

```rust
pub mod store;
```

Add to `src-tauri/src/runtime/mod.rs`:

```rust
pub mod expert_team;
```

Add to `AiJiaHome` in `src-tauri/src/storage/aijia_home.rs`:

```rust
pub fn expert_team_templates_cache_dir(&self) -> PathBuf {
    self.root.join("expert-team-templates-cache")
}
```

- [ ] **Step 4: Add Tauri catalog command**

Create `src-tauri/src/commands/expert_teams.rs`:

```rust
use crate::runtime::expert_team::store::{bootstrap_teams, ExpertTeamSnapshot};

#[tauri::command]
pub async fn expert_team_template_catalog() -> Result<Vec<ExpertTeamSnapshot>, String> {
    bootstrap_teams().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn expert_team_upgrade_conversation(
    _conversation_id: String,
    _target_version: String,
) -> Result<(), String> {
    Err("专家团升级将在远程快照同步完成后启用".to_string())
}
```

Add to `commands/mod.rs`:

```rust
pub mod expert_teams;
```

Register in `lib.rs`:

```rust
commands::expert_teams::expert_team_template_catalog,
commands::expert_teams::expert_team_upgrade_conversation,
```

- [ ] **Step 5: Run Rust tests**

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
cargo test expert_team::store --lib
cargo check
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cd /Users/gezhigang/work-codeup/aijia/code
git add src-tauri/src/runtime/expert_team src-tauri/src/runtime/mod.rs src-tauri/src/storage/aijia_home.rs src-tauri/src/commands/expert_teams.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat: add expert team snapshot store"
```

## Task 9: Frontend Tauri Types and Expert-Team Catalog Hook

**Files:**
- Modify: `src/lib/tauri.ts`
- Create: `src/features/expert-teams/useExpertTeamCatalog.ts`
- Modify: `src/features/expert-teams/teams.ts`
- Test: `src/features/expert-teams/__tests__/useExpertTeamCatalog.test.tsx`

- [ ] **Step 1: Add frontend test for bootstrap fallback**

Create `src/features/expert-teams/__tests__/useExpertTeamCatalog.test.tsx`:

```tsx
import { renderHook, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

vi.mock('@/lib/tauri', () => ({
  expertTeamTemplateCatalog: vi.fn(async () => {
    throw new Error('ipc unavailable')
  }),
}))

import { useExpertTeamCatalog } from '../useExpertTeamCatalog'

describe('useExpertTeamCatalog', () => {
  it('falls back to builtin expert teams when IPC fails', async () => {
    const { result } = renderHook(() => useExpertTeamCatalog())
    await waitFor(() => expect(result.current.isLoading).toBe(false))
    expect(result.current.teams.length).toBeGreaterThan(0)
    expect(result.current.source).toBe('bootstrap')
  })
})
```

- [ ] **Step 2: Run failing frontend test**

```bash
cd /Users/gezhigang/work-codeup/aijia/code
pnpm exec vitest run src/features/expert-teams/__tests__/useExpertTeamCatalog.test.tsx
```

Expected: FAIL because `useExpertTeamCatalog` does not exist.

- [ ] **Step 3: Add Tauri types and wrapper**

In `src/lib/tauri.ts`, add:

```ts
export interface LocalizedExpertTeamDisplay {
  name: string
  tagline?: string
  examples?: string[]
  composerPlaceholder?: string
}

export interface ExpertPersonaSnapshot {
  stableName: string
  emoji?: string
  displayI18n?: Record<string, { name: string }>
  promptI18n?: Record<string, { persona: string }>
}

export interface ExpertTeamSnapshot {
  teamId: string
  version: string
  facilitationStyle: 'rounds' | 'debate' | 'open'
  displayI18n: Record<string, LocalizedExpertTeamDisplay>
  experts: ExpertPersonaSnapshot[]
  directorPromptI18n: Record<string, { template: string }>
}

export function expertTeamTemplateCatalog(): Promise<ExpertTeamSnapshot[]> {
  return invoke<ExpertTeamSnapshot[]>('expert_team_template_catalog')
}
```

- [ ] **Step 4: Convert `teams.ts` to export bootstrap fallback and mapper**

In `src/features/expert-teams/teams.ts`, keep the existing `EXPERT_TEAMS` array but rename export to:

```ts
export const BUILTIN_EXPERT_TEAMS: ExpertTeam[] = [
  // existing entries
]

export const EXPERT_TEAMS = BUILTIN_EXPERT_TEAMS
```

Add mapper:

```ts
import i18n from '@/i18n'
import type { ExpertTeamSnapshot } from '@/lib/tauri'

export function snapshotToExpertTeam(snapshot: ExpertTeamSnapshot): ExpertTeam {
  const lang = i18n.language === 'en-US' ? 'en-US' : 'zh-CN'
  const display = snapshot.displayI18n[lang] ?? snapshot.displayI18n['zh-CN']
  return {
    id: snapshot.teamId,
    name: display.name,
    emoji: snapshot.experts[0]?.emoji ?? '🧠',
    tagline: display.tagline ?? '',
    experts: snapshot.experts.map((expert) => ({
      name: expert.displayI18n?.[lang]?.name ?? expert.displayI18n?.['zh-CN']?.name ?? expert.stableName,
      agentName: expert.stableName,
      persona: expert.promptI18n?.[lang]?.persona ?? expert.promptI18n?.['zh-CN']?.persona ?? '',
      emoji: expert.emoji ?? '🧠',
    })),
    examples: display.examples ?? [],
    composerPlaceholder: display.composerPlaceholder ?? '',
    facilitationStyle: snapshot.facilitationStyle,
    snapshot,
  }
}
```

Update the `ExpertTeam` interface to include:

```ts
snapshot?: ExpertTeamSnapshot
```

Change `ExpertTeamId` type from the closed string union to:

```ts
export type ExpertTeamId = string
```

- [ ] **Step 5: Implement catalog hook**

Create `src/features/expert-teams/useExpertTeamCatalog.ts`:

```ts
import { useEffect, useState } from 'react'

import { expertTeamTemplateCatalog } from '@/lib/tauri'
import { BUILTIN_EXPERT_TEAMS, snapshotToExpertTeam, type ExpertTeam } from './teams'

export type ExpertTeamCatalogSource = 'remote' | 'bootstrap'

export function useExpertTeamCatalog() {
  const [teams, setTeams] = useState<ExpertTeam[]>(BUILTIN_EXPERT_TEAMS)
  const [source, setSource] = useState<ExpertTeamCatalogSource>('bootstrap')
  const [isLoading, setIsLoading] = useState(true)

  useEffect(() => {
    let cancelled = false
    ;(async () => {
      try {
        const snapshots = await expertTeamTemplateCatalog()
        if (!cancelled && snapshots.length > 0) {
          setTeams(snapshots.map(snapshotToExpertTeam))
          setSource('remote')
        }
      } catch (err) {
        console.warn('[expert-teams] catalog load failed, using bootstrap:', err)
      } finally {
        if (!cancelled) setIsLoading(false)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])

  return { teams, source, isLoading }
}
```

- [ ] **Step 6: Run frontend hook test**

```bash
cd /Users/gezhigang/work-codeup/aijia/code
pnpm exec vitest run src/features/expert-teams/__tests__/useExpertTeamCatalog.test.tsx
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
cd /Users/gezhigang/work-codeup/aijia/code
git add src/lib/tauri.ts src/features/expert-teams/teams.ts src/features/expert-teams/useExpertTeamCatalog.ts src/features/expert-teams/__tests__/useExpertTeamCatalog.test.tsx
git commit -m "feat: load expert teams from desktop catalog"
```

## Task 10: Remote Expert-Team Page and Prompt Rendering

**Files:**
- Modify: `src/features/expert-teams/ExpertTeamsPage.tsx`
- Modify: `src/features/expert-teams/buildDirectorPrompt.ts`
- Modify: `src/components/chat-scene/ChatBottomArea.tsx`
- Modify: `src/components/chat-scene/ExpertTeamWelcome.tsx`
- Test: `src/features/expert-teams/__tests__/buildDirectorPrompt.test.ts`

- [ ] **Step 1: Add English director prompt test**

Append to `src/features/expert-teams/__tests__/buildDirectorPrompt.test.ts`:

```ts
it('uses snapshot English director prompt when provided', () => {
  const team = {
    id: 'strategy',
    name: 'Strategy Simulation Team',
    emoji: '🎯',
    tagline: 'Stress-test decisions',
    experts: [
      { name: 'CFO', agentName: 'cfo', persona: 'Focuses on ROI', emoji: '💰' },
    ],
    examples: [],
    composerPlaceholder: '',
    facilitationStyle: 'rounds' as const,
    snapshot: {
      teamId: 'strategy',
      version: '1.0.0',
      facilitationStyle: 'rounds' as const,
      displayI18n: {
        'en-US': { name: 'Strategy Simulation Team' },
        'zh-CN': { name: '战略推演团' },
      },
      experts: [],
      directorPromptI18n: {
        'en-US': { template: 'Facilitate {teamName} for {topic}. Members: {roster}' },
        'zh-CN': { template: '主持 {teamName}: {topic}. 成员：{roster}' },
      },
    },
  }
  const prompt = buildDirectorPrompt(team, 'market expansion', 'en-US')
  expect(prompt).toContain('Facilitate Strategy Simulation Team for market expansion')
})
```

- [ ] **Step 2: Run failing prompt test**

```bash
cd /Users/gezhigang/work-codeup/aijia/code
pnpm exec vitest run src/features/expert-teams/__tests__/buildDirectorPrompt.test.ts
```

Expected: FAIL because `buildDirectorPrompt` does not accept language or snapshot templates.

- [ ] **Step 3: Update `buildDirectorPrompt`**

Modify signature:

```ts
export function buildDirectorPrompt(team: ExpertTeam, userTopic: string, language?: string): string
```

Add helper:

```ts
function renderSnapshotPrompt(team: ExpertTeam, topic: string, language?: string): string | null {
  const template = team.snapshot?.directorPromptI18n?.[language === 'en-US' ? 'en-US' : 'zh-CN']?.template
    ?? team.snapshot?.directorPromptI18n?.['zh-CN']?.template
  if (!template) return null
  return template
    .replaceAll('{teamName}', team.name)
    .replaceAll('{topic}', topic)
    .replaceAll('{roster}', renderRoster(team))
}
```

At top of `buildDirectorPrompt`:

```ts
const fromSnapshot = renderSnapshotPrompt(team, topic, language)
if (fromSnapshot) return fromSnapshot
```

- [ ] **Step 4: Use remote catalog on ExpertTeamsPage**

In `ExpertTeamsPage.tsx`, replace direct `EXPERT_TEAMS.map` with:

```tsx
const { teams } = useExpertTeamCatalog()
...
{teams.map((team) => (
  <ExpertTeamCard key={team.id} team={team} onStart={handleStart} />
))}
```

Import:

```ts
import { useExpertTeamCatalog } from './useExpertTeamCatalog'
```

- [ ] **Step 5: Pass current language into prompt builders**

In `ChatBottomArea.tsx`, import `useTranslation` or existing i18n instance and call:

```ts
markdownToSend = buildDirectorPrompt(team, markdownToSend, i18n.language)
```

In `ExpertTeamWelcome.tsx`, do the same for preview prompt generation.

- [ ] **Step 6: Run expert-team frontend tests**

```bash
cd /Users/gezhigang/work-codeup/aijia/code
pnpm exec vitest run src/features/expert-teams/__tests__/buildDirectorPrompt.test.ts src/features/expert-teams/__tests__/ExpertTeamCard.test.tsx src/features/expert-teams/__tests__/useExpertTeamCatalog.test.tsx
```

Expected: PASS. Update existing snapshots only if the change is intentional and the new snapshot contains the same semantics with language-aware additions.

- [ ] **Step 7: Commit**

```bash
cd /Users/gezhigang/work-codeup/aijia/code
git add src/features/expert-teams/ExpertTeamsPage.tsx src/features/expert-teams/buildDirectorPrompt.ts src/components/chat-scene/ChatBottomArea.tsx src/components/chat-scene/ExpertTeamWelcome.tsx src/features/expert-teams/__tests__/buildDirectorPrompt.test.ts src/features/expert-teams/__tests__/__snapshots__/buildDirectorPrompt.test.ts.snap
git commit -m "feat: render expert team prompts from snapshots"
```

## Task 11: Expert-Team Conversation Freeze

**Files:**
- Modify: `src-tauri/src/runtime/expert_team/store.rs`
- Modify: `src-tauri/src/commands/expert_teams.rs`
- Modify: `src-tauri/src/commands/chat.rs`
- Test: `src-tauri/src/runtime/expert_team/store.rs`

- [ ] **Step 1: Add freeze path tests**

Append to `src-tauri/src/runtime/expert_team/store.rs` tests:

```rust
#[test]
fn freeze_snapshot_writes_template_json() {
    let tmp = tempfile::tempdir().unwrap();
    let snapshot = ExpertTeamSnapshot {
        team_id: "strategy".into(),
        version: "1.0.0".into(),
        facilitation_style: "rounds".into(),
        display_i18n: BTreeMap::new(),
        experts: vec![],
        director_prompt_i18n: BTreeMap::new(),
    };
    freeze_conversation_snapshot(tmp.path(), &snapshot).expect("freeze ok");
    assert!(tmp.path().join("expert-team/template.json").is_file());
}
```

- [ ] **Step 2: Run failing Rust test**

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
cargo test expert_team::store::tests::freeze_snapshot_writes_template_json --lib
```

Expected: FAIL because `freeze_conversation_snapshot` is undefined.

- [ ] **Step 3: Implement freeze/read helpers**

Add to `store.rs`:

```rust
pub fn conversation_template_dir(conv_dir: &Path) -> PathBuf {
    conv_dir.join("expert-team")
}

pub fn conversation_template_path(conv_dir: &Path) -> PathBuf {
    conversation_template_dir(conv_dir).join("template.json")
}

pub fn freeze_conversation_snapshot(conv_dir: &Path, snapshot: &ExpertTeamSnapshot) -> Result<()> {
    let dir = conversation_template_dir(conv_dir);
    fs::create_dir_all(&dir)?;
    fs::write(conversation_template_path(conv_dir), serde_json::to_vec_pretty(snapshot)?)?;
    Ok(())
}

pub fn read_conversation_snapshot(conv_dir: &Path) -> Result<Option<ExpertTeamSnapshot>> {
    let path = conversation_template_path(conv_dir);
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(&path)?;
    Ok(Some(serde_json::from_slice(&bytes)?))
}
```

- [ ] **Step 4: Freeze when setting expert team source**

Find `set_conversation_expert_team` in `src-tauri/src/commands/chat.rs`. After it records source metadata, resolve the conversation directory and call `freeze_conversation_snapshot` for the selected team snapshot. If only bootstrap lookup exists at this stage, choose the matching `bootstrap_teams()` entry by `team_id`.

Use this error behavior:

```rust
if let Err(err) = freeze_result {
    log::warn!("[expert-team] freeze snapshot failed for conv={}: {}", conversation_id, err);
}
```

Do not fail conversation creation if freeze fails; the frontend can still fallback to bootstrap, and diagnostics will show the issue.

- [ ] **Step 5: Run Rust tests**

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
cargo test expert_team::store --lib
cargo check
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cd /Users/gezhigang/work-codeup/aijia/code
git add src-tauri/src/runtime/expert_team/store.rs src-tauri/src/commands/expert_teams.rs src-tauri/src/commands/chat.rs
git commit -m "feat: freeze expert team snapshots per conversation"
```

## Task 12: Manual Upgrade Affordance Shell

**Files:**
- Modify: `src-tauri/src/runtime/employee/template_store.rs`
- Modify: `src-tauri/src/commands/employees.rs`
- Modify: `src-tauri/src/commands/expert_teams.rs`
- Modify: `src/lib/tauri.ts`
- Modify: `src/features/employees/EmployeeDrawer.tsx`
- Modify: `src/components/chat-scene/ExpertTeamWelcome.tsx`
- Modify: `src/i18n/zh-CN.json`
- Modify: `src/i18n/en-US.json`

- [ ] **Step 1: Add Rust upgrade check test for employee versions**

Append to `template_store.rs` tests:

```rust
#[test]
fn version_is_newer_uses_numeric_segments() {
    assert!(is_newer_version("1.10.0", "1.2.0"));
    assert!(!is_newer_version("1.2.0", "1.10.0"));
}
```

- [ ] **Step 2: Implement version helper**

Add to `template_store.rs`, reusing the catalog comparator from Task 7:

```rust
use crate::runtime::desktop_resources::catalog::compare_versions;
use std::cmp::Ordering;

pub fn is_newer_version(candidate: &str, current: &str) -> bool {
    compare_versions(candidate, current) == Ordering::Greater
}
```

- [ ] **Step 3: Add command shells**

In `commands/employees.rs`, add:

```rust
#[tauri::command]
pub async fn employee_template_check_upgrade(
    app: AppHandle,
    id: String,
) -> Result<Option<TemplateSnapshot>, String> {
    let store = employee_store(&app)?;
    let record = store.get(&id).map_err(|e| e.to_string())?;
    let Some(template_ref) = record.template_ref else {
        return Ok(None);
    };
    let cache_dir = AiJiaHome::from_home().employee_templates_cache_dir();
    Ok(find_latest_for_template(&cache_dir, &template_ref.template_id)
        .filter(|latest| is_newer_version(&latest.version, &template_ref.version)))
}
```

If `employee_template_check_upgrade` already exists, adjust it to use bilingual `TemplateSnapshot` without changing its IPC name.

In `commands/expert_teams.rs`, replace the temporary upgrade error with:

```rust
#[tauri::command]
pub async fn expert_team_upgrade_conversation(
    conversation_id: String,
    target_version: String,
) -> Result<(), String> {
    log::info!(
        "[expert-team] upgrade requested conv={} target_version={}",
        conversation_id,
        target_version
    );
    Err("当前版本仅支持检测专家团新版本，升级写入将在快照同步完成后启用".to_string())
}
```

This step intentionally gives the frontend a stable command name before full cache-backed upgrade is implemented.

- [ ] **Step 4: Add frontend wrappers**

In `src/lib/tauri.ts`, add:

```ts
export function employeeTemplateCheckUpgrade(id: string): Promise<EmployeeTemplateSnapshot | null> {
  return invoke<EmployeeTemplateSnapshot | null>('employee_template_check_upgrade', { id })
}

export function expertTeamUpgradeConversation(conversationId: string, targetVersion: string): Promise<void> {
  return invoke<void>('expert_team_upgrade_conversation', { conversationId, targetVersion })
}
```

- [ ] **Step 5: Add i18n strings**

In `src/i18n/zh-CN.json` under a resource/employee/expert-team section:

```json
"resourceUpdates": {
  "newVersion": "有新版本",
  "upgrade": "升级",
  "upgradeUnavailable": "当前正在运行，完成后可升级"
}
```

In `src/i18n/en-US.json`:

```json
"resourceUpdates": {
  "newVersion": "New version available",
  "upgrade": "Upgrade",
  "upgradeUnavailable": "A run is active. Upgrade after it finishes."
}
```

- [ ] **Step 6: Run focused checks**

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
cargo test runtime::employee::template_store --lib
cargo check

cd /Users/gezhigang/work-codeup/aijia/code
pnpm exec vitest run src/features/employees/templates.test.ts src/features/expert-teams/__tests__/useExpertTeamCatalog.test.tsx
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
cd /Users/gezhigang/work-codeup/aijia/code
git add src-tauri/src/runtime/employee/template_store.rs src-tauri/src/commands/employees.rs src-tauri/src/commands/expert_teams.rs src/lib/tauri.ts src/i18n/zh-CN.json src/i18n/en-US.json
git commit -m "feat: add desktop resource upgrade affordance shell"
```

## Task 13: End-to-End Verification

**Files:**
- No planned source changes unless a verification failure exposes a bug.

- [ ] **Step 1: Verify Lotus shared**

```bash
cd /Users/gezhigang/lotus/code/shared
go test ./...
```

Expected: PASS.

- [ ] **Step 2: Verify Lotus gateway**

```bash
cd /Users/gezhigang/lotus/code/api-gateway
go test ./...
```

Expected: PASS.

- [ ] **Step 3: Verify Lotus OPS server**

```bash
cd /Users/gezhigang/lotus/code/ops-portal/server
go test ./...
```

Expected: PASS.

- [ ] **Step 4: Verify desktop Rust**

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
cargo check
cargo test expert_team::store --lib
cargo test desktop_resources::catalog --lib
```

Expected: PASS.

- [ ] **Step 5: Verify desktop frontend**

```bash
cd /Users/gezhigang/work-codeup/aijia/code
pnpm exec vitest run src/features/expert-teams/__tests__/buildDirectorPrompt.test.ts src/features/expert-teams/__tests__/ExpertTeamCard.test.tsx src/features/expert-teams/__tests__/useExpertTeamCatalog.test.tsx src/features/employees/templates.test.ts
pnpm lint
```

Expected: PASS.

- [ ] **Step 6: Record final status**

```bash
cd /Users/gezhigang/work-codeup/aijia/code
git status --short --branch

cd /Users/gezhigang/lotus
git status --short --branch
```

Expected:

- Only intended commits/working-tree changes remain.
- Any unrelated pre-existing files are listed separately in the final handoff.
