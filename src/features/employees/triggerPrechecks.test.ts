import { describe, expect, it } from 'vitest'
import { runTriggerPrechecks } from './triggerPrechecks'
import type { EmployeeRecord } from '@/lib/tauri'
import type { EmployeeTemplate } from './templates'

const baseEmployee: EmployeeRecord = {
  id: 'emp-1',
  name: 'X',
  role: 'r',
  description: 'd',
  avatar: '🤖',
  templateId: 'builtin:test',
  toolWhitelist: [],
  cron: null,
  timezone: 'Asia/Shanghai',
  enabled: true,
  resourceConfig: {},
  systemPromptExtra: null,
  defaultSkillId: null,
  createdAt: '',
  updatedAt: '',
  lastRunAt: null,
  nextRunAt: null,
}

const baseTemplate: EmployeeTemplate = {
  templateId: 'builtin:test',
  avatar: '🤖',
  name: 'X',
  role: 'r',
  description: 'd',
  toolWhitelist: [],
  cron: null,
  systemPromptExtra: '',
  badge: '',
  defaultSkillId: null,
  requiresAttachment: null,
  resourceConfigKind: 'none',
  requiresDingtalk: false,
}

describe('runTriggerPrechecks', () => {
  it('returns ready when nothing is required', () => {
    expect(
      runTriggerPrechecks({ template: baseTemplate, employee: baseEmployee, dingtalkConnected: false }),
    ).toEqual({ kind: 'ready' })
  })

  it('asks for attachments when template.requiresAttachment is set', () => {
    expect(
      runTriggerPrechecks({
        template: { ...baseTemplate, requiresAttachment: { accept: '.pdf', min: 1, max: 5 } },
        employee: baseEmployee,
        dingtalkConnected: false,
      }),
    ).toEqual({
      kind: 'attachments',
      spec: { accept: '.pdf', min: 1, max: 5 },
    })
  })

  it('asks for resource_config when kind is monitoring-urls and config is empty', () => {
    expect(
      runTriggerPrechecks({
        template: { ...baseTemplate, resourceConfigKind: 'monitoring-urls' },
        employee: baseEmployee,
        dingtalkConnected: false,
      }),
    ).toEqual({ kind: 'resource', resourceConfigKind: 'monitoring-urls' })
  })

  it('treats monitoringTargets array of length >= 1 as configured', () => {
    expect(
      runTriggerPrechecks({
        template: { ...baseTemplate, resourceConfigKind: 'monitoring-urls' },
        employee: {
          ...baseEmployee,
          resourceConfig: { monitoringTargets: [{ name: 'A', url: 'https://a' }] },
        },
        dingtalkConnected: false,
      }),
    ).toEqual({ kind: 'ready' })
  })

  it('asks for dingtalk when template requires it and not connected', () => {
    expect(
      runTriggerPrechecks({
        template: { ...baseTemplate, requiresDingtalk: true },
        employee: baseEmployee,
        dingtalkConnected: false,
      }),
    ).toEqual({ kind: 'dingtalk' })
  })

  it('skips dingtalk precheck when connected', () => {
    expect(
      runTriggerPrechecks({
        template: { ...baseTemplate, requiresDingtalk: true },
        employee: baseEmployee,
        dingtalkConnected: true,
      }),
    ).toEqual({ kind: 'ready' })
  })

  it('precedence: attachments > resource > dingtalk', () => {
    expect(
      runTriggerPrechecks({
        template: {
          ...baseTemplate,
          resourceConfigKind: 'sales-table',
          requiresDingtalk: true,
          requiresAttachment: null,
        },
        employee: baseEmployee,
        dingtalkConnected: false,
      }),
    ).toEqual({ kind: 'resource', resourceConfigKind: 'sales-table' })
  })
})
