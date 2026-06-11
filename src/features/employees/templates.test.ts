import { describe, expect, it } from 'vitest'

import type { EmployeeTemplateSnapshot } from '@/lib/tauri'

import { getEmployeeVisual } from './employeeVisual'
import { snapshotToTemplate } from './templates'

function makeSnapshot(overrides: Partial<EmployeeTemplateSnapshot> = {}): EmployeeTemplateSnapshot {
  return {
    templateId: 'builtin:xiaoyuan',
    version: '1.0.0',
    name: '小研',
    avatar: '🔍',
    role: '行业/竞品调研员',
    description: 'd',
    badge: '🟢',
    systemPromptExtra: '',
    toolWhitelist: ['WebSearch'],
    cron: '',
    defaultSkillId: '',
    requiresDingtalk: false,
    requiresAttachment: null,
    resourceConfigSchema: null,
    resourceConfigUI: null,
    ...overrides,
  }
}

describe('snapshotToTemplate', () => {
  it('uses catalog fields and version when a builtin id comes from the server catalog', () => {
    const snap = makeSnapshot({ templateId: 'builtin:xiaoyuan', name: '远程小研', version: '1.2.0' })
    const out = snapshotToTemplate(snap)
    expect(out.name).toBe('远程小研')
    expect(out.version).toBe('1.2.0')
    expect(out.resourceConfigKind).toBe('monitoring-urls')
  })

  it('maps an unknown templateId to a synthesized EmployeeTemplate', () => {
    const snap = makeSnapshot({
      templateId: 'org:acme-recruiter',
      version: '2.3.4',
      name: '招聘助理',
      cron: '0 9 * * 1',
      defaultSkillId: 'resume-screening',
      requiresAttachment: { accept: '.pdf', min: 1, max: 5 },
      requiresDingtalk: true,
    })
    const out = snapshotToTemplate(snap)
    expect(out.templateId).toBe('org:acme-recruiter')
    expect(out.version).toBe('2.3.4')
    expect(out.name).toBe('招聘助理')
    expect(out.cron).toBe('0 9 * * 1')
    expect(out.defaultSkillId).toBe('resume-screening')
    expect(out.requiresAttachment).toEqual({ accept: '.pdf', min: 1, max: 5 })
    expect(out.requiresDingtalk).toBe(true)
    // Unknown templates default to 'none' until they ship with a schema (PR6).
    expect(out.resourceConfigKind).toBe('none')
  })

  it('maps empty cron and empty defaultSkillId to null', () => {
    const snap = makeSnapshot({
      templateId: 'org:custom',
      cron: '',
      defaultSkillId: '',
    })
    const out = snapshotToTemplate(snap)
    expect(out.cron).toBeNull()
    expect(out.defaultSkillId).toBeNull()
  })

  it('keeps the catalog version for builtin snapshots', () => {
    const snap = makeSnapshot({
      templateId: 'builtin:xiaoyuan',
      version: '1.2.0',
    })
    const out = snapshotToTemplate(snap)
    expect(out.version).toBe('1.2.0')
  })

  it('uses displayI18n fields for the requested locale', () => {
    const snap = makeSnapshot({
      displayI18n: {
        'en-US': {
          name: 'Researcher',
          role: 'Market research analyst',
          description: 'Tracks competitors weekly.',
          badge: 'Ready',
        },
      },
    })
    const out = snapshotToTemplate(snap, 'en-US')
    expect(out.name).toBe('Researcher')
    expect(out.role).toBe('Market research analyst')
    expect(out.description).toBe('Tracks competitors weekly.')
    expect(out.badge).toBe('Ready')
  })

  it('uses remote avatar fields from localized displayI18n', () => {
    const snap = makeSnapshot({
      templateId: 'org:salary-expert',
      displayI18n: {
        'zh-CN': {
          name: '方予衡',
          role: '薪酬专家',
          description: '读取薪酬数据。',
          badge: '平台技能',
          avatarAssetKey: 'desktop-resources/employee-avatars/hr-v1/salary-expert.svg',
          avatarUrl: 'https://lotus-releases.oss-cn-beijing.aliyuncs.com/desktop-resources/employee-avatars/hr-v1/salary-expert.svg',
        },
      },
    })
    const out = snapshotToTemplate(snap, 'zh-CN')
    expect(out.avatarAssetKey).toBe('desktop-resources/employee-avatars/hr-v1/salary-expert.svg')
    expect(out.avatarUrl).toBe('https://lotus-releases.oss-cn-beijing.aliyuncs.com/desktop-resources/employee-avatars/hr-v1/salary-expert.svg')
    expect(getEmployeeVisual(out).avatarUrl).toBe('/employee-avatars/方予衡.svg')
  })

  it('derives a public release avatar URL from avatarAssetKey when avatarUrl is absent', () => {
    const snap = makeSnapshot({
      templateId: 'org:attendance-expert',
      avatarAssetKey: 'desktop-resources/employee-avatars/hr-v1/attendance-expert.svg',
    })
    const out = snapshotToTemplate(snap)
    expect(out.avatarUrl).toBe('https://lotus-releases.oss-cn-beijing.aliyuncs.com/desktop-resources/employee-avatars/hr-v1/attendance-expert.svg')
    expect(getEmployeeVisual(out).avatarUrl).toBe(out.avatarUrl)
  })
})
