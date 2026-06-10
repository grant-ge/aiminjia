import type { SkillInfo } from '@/lib/tauri'

export type SkillCenterView = 'market' | 'builtin' | 'installed'

export const REQUIRED_BUILTIN_SKILL_IDS = new Set([
  'create-skill',
  'skill-creator',
  'dws',
  'dingtalk-workspace',
])

export function isSkillEnabled(skill: { enabled?: boolean }): boolean {
  return skill.enabled !== false
}

export function isBuiltinSkill(skill: Pick<SkillInfo, 'id' | 'source'>): boolean {
  return skill.source === 'builtin' || REQUIRED_BUILTIN_SKILL_IDS.has(skill.id)
}

export function isMarketSkill(skill: Pick<SkillInfo, 'id' | 'source'>): boolean {
  return !isBuiltinSkill(skill) && (skill.source === 'tenant' || skill.source === 'global')
}

export function isInstalledSkill(skill: Pick<SkillInfo, 'id' | 'source'>): boolean {
  return !isMarketSkill(skill) && !isBuiltinSkill(skill)
}

export function canToggleSkillEnablement(skill: Pick<SkillInfo, 'id' | 'source'>): boolean {
  return isBuiltinSkill(skill) || isInstalledSkill(skill)
}

export function skillMatchesCenterView(skill: SkillInfo, view: SkillCenterView): boolean {
  if (view === 'market') return isMarketSkill(skill)
  if (view === 'builtin') return isBuiltinSkill(skill)
  return isInstalledSkill(skill)
}
