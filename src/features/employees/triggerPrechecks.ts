import type { EmployeeRecord } from '@/lib/tauri'
import type { EmployeeTemplate, ResourceConfigKind } from './templates'

export type TriggerPrecheckResult =
  | { kind: 'ready' }
  | { kind: 'resource'; resourceConfigKind: ResourceConfigKind }
  | { kind: 'knowledge-indexing' }

export interface RunTriggerPrechecksParams {
  template: EmployeeTemplate
  employee: EmployeeRecord
}

/**
 * Decide what (if anything) the user must complete before we can call
 * employee_trigger.
 *
 * Order:
 *   resource_config (only when REQUIRED) → knowledge indexing
 *
 * **PR-10 (2026-05-15)**: the `attachments` precheck was removed. Previously
 * clicking 派活 on an employee like 小法 (合同审阅) immediately popped a
 * native file picker, which felt jarring and disconnected from the chat
 * flow. The new design opens the chat first; the LLM's first turn (driven
 * by a prompt hint we add when the template requires attachments but
 * none were supplied) asks the user to drag-drop or paste the files
 * into the conversation. Attachments may still be passed programmatically
 * (e.g. by future skills that pre-fetch documents).
 *
 * Note on resource_config: only `monitoring-urls` is a HARD requirement (小研
 * needs at least one URL to do anything). `sales-table` is a SOFT requirement
 * — the employee can fall back to asking the user inside the chat (path A) and
 * persisting the answers to memory; the ResourceConfigForm at ⚙️ is a faster
 * path B that pre-fills the same shape.
 *
 * Dingtalk auth is NOT a precheck: the `dingtalk-workspace` skill checks
 * `dws auth status` at the start of every turn and walks the user through
 * scanning a QR code inside the conversation when not authenticated. Hard
 * gating here would short-circuit the skill's own lazy-auth flow.
 *
 * Returns `{ kind: 'ready' }` when no precheck is required.
 */
export function runTriggerPrechecks(params: RunTriggerPrechecksParams): TriggerPrecheckResult {
  const { template, employee } = params

  if (
    template.resourceConfigKind !== 'none' &&
    isResourceConfigRequired(template) &&
    !isResourceConfigured(template, employee)
  ) {
    return { kind: 'resource', resourceConfigKind: template.resourceConfigKind }
  }

  if (hasPendingKnowledgeSources(employee)) {
    return { kind: 'knowledge-indexing' }
  }

  return { kind: 'ready' }
}

function hasPendingKnowledgeSources(employee: EmployeeRecord): boolean {
  const cfg = employee.resourceConfig as Record<string, unknown> | null | undefined
  const sources = cfg?.knowledgeSources
  if (!Array.isArray(sources)) return false
  return sources.some((s) => {
    if (!s || typeof s !== 'object') return false
    const status = (s as Record<string, unknown>).status
    return status === 'pending' || status === 'indexing'
  })
}

/** Whether the resource_config is mandatory before dispatch. */
function isResourceConfigRequired(template: EmployeeTemplate): boolean {
  switch (template.resourceConfigKind) {
    case 'monitoring-urls':
      return true
    case 'weekly-report':
      return true
    case 'sales-table':
      // Soft requirement: employee can ask the user inside chat and persist
      // to memory (path A). The form is a convenience.
      return false
    case 'tech-support':
      // Soft: employee can work without pre-config — will scan all groups
      return false
    case 'customer-support':
      return false
    case 'none':
      return false
  }
}

function isResourceConfigured(template: EmployeeTemplate, employee: EmployeeRecord): boolean {
  const cfg = employee.resourceConfig as Record<string, unknown> | null | undefined
  if (!cfg) return false

  switch (template.resourceConfigKind) {
    case 'monitoring-urls': {
      const targets = cfg.monitoringTargets
      return Array.isArray(targets) && targets.length > 0
    }
    case 'sales-table': {
      // Configured = at least the table identifier is set. Field mapping is
      // optional at this gate; the SKILL completes whatever's missing.
      const baseId = cfg.baseId
      const tableId = cfg.tableId
      return typeof baseId === 'string' && baseId.length > 0
        && typeof tableId === 'string' && tableId.length > 0
    }
    case 'weekly-report': {
      // Configured = template selected (defaults to 'standard' on first save).
      const tpl = cfg.template
      return typeof tpl === 'string' && tpl.length > 0
    }
    case 'tech-support': {
      // Configured = at least groupMatch keywords are set
      const gm = cfg.groupMatch as Record<string, unknown> | undefined
      return !!gm && Array.isArray(gm.keywords) && gm.keywords.length > 0
    }
    case 'customer-support': {
      const gm = cfg.groupMatch as Record<string, unknown> | undefined
      return !!gm && Array.isArray(gm.keywords) && gm.keywords.length > 0
    }
    case 'none':
      return true
  }
}
