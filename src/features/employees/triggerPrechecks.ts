import type { EmployeeRecord } from '@/lib/tauri'
import type { EmployeeTemplate, RequiresAttachmentSpec, ResourceConfigKind } from './templates'

export type TriggerPrecheckResult =
  | { kind: 'ready' }
  | { kind: 'attachments'; spec: RequiresAttachmentSpec }
  | { kind: 'resource'; resourceConfigKind: ResourceConfigKind }
  | { kind: 'dingtalk' }

export interface RunTriggerPrechecksParams {
  template: EmployeeTemplate
  employee: EmployeeRecord
  /** Result of `dingtalkStatus().connected`. Caller fetches before invoking. */
  dingtalkConnected: boolean
}

/**
 * Decide what (if anything) the user must complete before we can call
 * employee_trigger. Order: attachments first (per-trigger, cheapest),
 * then resource_config (per-employee, persisted), then dingtalk auth
 * (per-app, persisted).
 *
 * Returns `{ kind: 'ready' }` when no precheck is required.
 */
export function runTriggerPrechecks(params: RunTriggerPrechecksParams): TriggerPrecheckResult {
  const { template, employee, dingtalkConnected } = params

  if (template.requiresAttachment) {
    return { kind: 'attachments', spec: template.requiresAttachment }
  }

  if (template.resourceConfigKind !== 'none' && !isResourceConfigured(template, employee)) {
    return { kind: 'resource', resourceConfigKind: template.resourceConfigKind }
  }

  if (template.requiresDingtalk && !dingtalkConnected) {
    return { kind: 'dingtalk' }
  }

  return { kind: 'ready' }
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
      // Stub: never considered configured in this MVP — keeps 小销 in 🟠 state.
      return false
    }
    case 'none':
      return true
  }
}
