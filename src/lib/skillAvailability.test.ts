import { describe, expect, it } from 'vitest'

import {
  canToggleSkillEnablement,
  isBuiltinSkill,
  isInstalledSkill,
  isMarketSkill,
  skillMatchesCenterView,
} from '@/lib/skillAvailability'
import type { SkillInfo } from '@/lib/tauri'

function skill(id: string, source: SkillInfo['source']): SkillInfo {
  return {
    id,
    source,
    displayName: id,
    displayNameEn: id,
    description: '',
    shortDescription: '',
    shortDescriptionEn: '',
    icon: '',
    category: 'general',
    triggerText: `/${id}`,
    hasWorkflow: false,
    updatedAt: null,
    enabled: true,
  }
}

describe('skillAvailability', () => {
  it('classifies browser as a required builtin skill even when it is synced from global storage', () => {
    const browser = skill('browser', 'global')

    expect(isBuiltinSkill(browser)).toBe(true)
    expect(isMarketSkill(browser)).toBe(false)
    expect(isInstalledSkill(browser)).toBe(false)
    expect(skillMatchesCenterView(browser, 'builtin')).toBe(true)
    expect(skillMatchesCenterView(browser, 'installed')).toBe(false)
  })

  it('classifies find-skills as a required builtin skill that remains toggleable', () => {
    const findSkills = skill('find-skills', 'global')

    expect(isBuiltinSkill(findSkills)).toBe(true)
    expect(isMarketSkill(findSkills)).toBe(false)
    expect(isInstalledSkill(findSkills)).toBe(false)
    expect(canToggleSkillEnablement(findSkills)).toBe(true)
    expect(skillMatchesCenterView(findSkills, 'builtin')).toBe(true)
    expect(skillMatchesCenterView(findSkills, 'installed')).toBe(false)
  })
})
