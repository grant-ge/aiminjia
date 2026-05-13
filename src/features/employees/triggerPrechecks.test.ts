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
  lifecycle: 'active',
  cronEnabled: true,
  resourceConfig: {},
  systemPromptExtra: null,
  defaultSkillId: null,
  templateRef: null,
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
      runTriggerPrechecks({ template: baseTemplate, employee: baseEmployee }),
    ).toEqual({ kind: 'ready' })
  })

  it('asks for attachments when template.requiresAttachment is set', () => {
    expect(
      runTriggerPrechecks({
        template: { ...baseTemplate, requiresAttachment: { accept: '.pdf', min: 1, max: 5 } },
        employee: baseEmployee,
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
      }),
    ).toEqual({ kind: 'ready' })
  })

  it('never gates on dingtalk auth — the skill handles QR-code login in-chat', () => {
    // dingtalk auth used to be a hard precheck. It's now fully delegated to
    // the `dingtalk-workspace` skill which runs `dws auth status` at the
    // start of every turn and walks the user through scanning a QR code
    // inside the conversation.
    expect(
      runTriggerPrechecks({
        template: { ...baseTemplate, requiresDingtalk: true },
        employee: baseEmployee,
      }),
    ).toEqual({ kind: 'ready' })
  })

  it('precedence: attachments > (hard) resource', () => {
    expect(
      runTriggerPrechecks({
        template: {
          ...baseTemplate,
          resourceConfigKind: 'monitoring-urls',
          requiresAttachment: null,
        },
        employee: baseEmployee,
      }),
    ).toEqual({ kind: 'resource', resourceConfigKind: 'monitoring-urls' })
  })

  it('sales-table is a soft requirement: unconfigured does NOT block dispatch', () => {
    // 小销-shaped: with sales-table empty, prechecks should return 'ready' —
    // the SKILL handles the missing config inside the chat (path A).
    expect(
      runTriggerPrechecks({
        template: {
          ...baseTemplate,
          resourceConfigKind: 'sales-table',
          requiresAttachment: null,
        },
        employee: baseEmployee,
      }),
    ).toEqual({ kind: 'ready' })
  })

  it('sales-table is treated as configured when baseId+tableId present', () => {
    expect(
      runTriggerPrechecks({
        template: {
          ...baseTemplate,
          resourceConfigKind: 'sales-table',
        },
        employee: {
          ...baseEmployee,
          resourceConfig: { baseId: 'b1', tableId: 't1' },
        },
      }),
    ).toEqual({ kind: 'ready' })
  })

  it('blocks dispatch when knowledgeSources are still indexing', () => {
    expect(
      runTriggerPrechecks({
        template: { ...baseTemplate, resourceConfigKind: 'customer-support' },
        employee: {
          ...baseEmployee,
          resourceConfig: {
            groupMatch: { keywords: ['x'] },
            knowledgeSources: [
              { path: '/tmp/a.md', originalName: 'a.md', status: 'indexing', slicedCount: 0 },
            ],
          },
        },
      }),
    ).toEqual({ kind: 'knowledge-indexing' })
  })

  it('blocks dispatch when knowledgeSources are still pending', () => {
    expect(
      runTriggerPrechecks({
        template: { ...baseTemplate, resourceConfigKind: 'customer-support' },
        employee: {
          ...baseEmployee,
          resourceConfig: {
            groupMatch: { keywords: ['x'] },
            knowledgeSources: [
              { path: '/tmp/a.md', originalName: 'a.md', status: 'pending', slicedCount: 0 },
            ],
          },
        },
      }),
    ).toEqual({ kind: 'knowledge-indexing' })
  })

  it('allows dispatch when knowledgeSources are done or failed', () => {
    expect(
      runTriggerPrechecks({
        template: { ...baseTemplate, resourceConfigKind: 'customer-support' },
        employee: {
          ...baseEmployee,
          resourceConfig: {
            groupMatch: { keywords: ['x'] },
            knowledgeSources: [
              { path: '/tmp/a.md', originalName: 'a.md', status: 'done', slicedCount: 12 },
              { path: '/tmp/b.md', originalName: 'b.md', status: 'failed', slicedCount: 0 },
            ],
          },
        },
      }),
    ).toEqual({ kind: 'ready' })
  })
})
