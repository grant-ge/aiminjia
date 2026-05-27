import { describe, expect, it } from 'vitest'

import type { EmployeeTemplateSnapshot } from '@/lib/tauri'

import { BUILTIN_TEMPLATES, snapshotToTemplate } from './templates'

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
  it('returns the verbatim BUILTIN_TEMPLATES entry when ids match', () => {
    // Same id ⇒ trust the hardcoded copy until PR5 removes it. This
    // means edits to the bootstrap JSON on the backend don't change UX
    // until the user upgrades the desktop app — that's intentional.
    const snap = makeSnapshot({ templateId: 'builtin:xiaoyuan', name: 'IGNORED' })
    const out = snapshotToTemplate(snap)
    const builtin = BUILTIN_TEMPLATES.find((t) => t.templateId === 'builtin:xiaoyuan')!
    expect(out).toBe(builtin)
  })

  it('maps an unknown templateId to a synthesized EmployeeTemplate', () => {
    const snap = makeSnapshot({
      templateId: 'org:acme-recruiter',
      name: '招聘助理',
      cron: '0 9 * * 1',
      defaultSkillId: 'resume-screening',
      requiresAttachment: { accept: '.pdf', min: 1, max: 5 },
      requiresDingtalk: true,
    })
    const out = snapshotToTemplate(snap)
    expect(out.templateId).toBe('org:acme-recruiter')
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

  it('uses localized remote fields for builtin templates when language changes', () => {
    const snap = makeSnapshot({
      templateId: 'builtin:xiaoyuan',
      displayI18n: {
        'zh-CN': {
          name: '小研',
          role: '行业/竞品调研员',
          description: '中文描述',
          badge: '中文徽章',
        },
        'en-US': {
          name: 'Research Analyst',
          role: 'Industry researcher',
          description: 'English description',
          badge: 'Ready to use',
        },
      },
      promptI18n: {
        'en-US': {
          systemPromptExtra: 'You are an industry research analyst.',
        },
      },
    } as unknown as Partial<EmployeeTemplateSnapshot>)

    const out = snapshotToTemplate(snap, 'en-US')

    expect(out.name).toBe('Research Analyst')
    expect(out.role).toBe('Industry researcher')
    expect(out.description).toBe('English description')
    expect(out.badge).toBe('Ready to use')
    expect(out.systemPromptExtra).toBe('You are an industry research analyst.')
  })
})
