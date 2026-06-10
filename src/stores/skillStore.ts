import { create } from 'zustand'

import type { SkillCategoryId } from '@/data/skill-categories'
import { isSkillEnabled } from '@/lib/skillAvailability'
import { installCustomSkill, listSkills, setSkillEnabled as setSkillEnabledIpc, uninstallCustomSkill, type SkillInfo } from '@/lib/tauri'

export { isSkillEnabled } from '@/lib/skillAvailability'

export type SkillValidationKind =
  | 'missingSkillMd'
  | 'parseFailed'
  | 'invalidName'
  | 'io'

export interface SkillValidationFailure {
  kind: SkillValidationKind
  detail?: string
}

export class SkillValidationError extends Error {
  readonly kind: SkillValidationKind
  readonly detail?: string
  constructor(failure: SkillValidationFailure) {
    super(`SKILL_VALIDATION:${failure.kind}`)
    this.name = 'SkillValidationError'
    this.kind = failure.kind
    this.detail = failure.detail
  }
}

export class SkillAlreadyExistsError extends Error {
  readonly skillId: string
  constructor(skillId: string) {
    super(`ALREADY_EXISTS:${skillId}`)
    this.skillId = skillId
    this.name = 'SkillAlreadyExistsError'
  }
}

const VALIDATION_KINDS: ReadonlySet<SkillValidationKind> = new Set([
  'missingSkillMd',
  'parseFailed',
  'invalidName',
  'io',
])

function toInstallError(err: unknown): Error {
  if (err && typeof err === 'object' && 'kind' in err) {
    const payload = err as { kind: string; detail?: string }
    if (payload.kind === 'alreadyExists') {
      return new SkillAlreadyExistsError(payload.detail ?? '')
    }
    if (VALIDATION_KINDS.has(payload.kind as SkillValidationKind)) {
      return new SkillValidationError({
        kind: payload.kind as SkillValidationKind,
        detail: payload.detail,
      })
    }
  }
  return err instanceof Error ? err : new Error(String(err))
}

const RECOMMENDED_SKILL_IDS = ['salary-benchmarking', 'biz-writing', 'contract-review', 'org-diagnosis']

type SkillInfoFromBackend = Omit<SkillInfo, 'enabled'> & { enabled?: boolean }

export function selectEnabledSkills(state: { skills: SkillInfo[] }): SkillInfo[] {
  return state.skills.filter(isSkillEnabled)
}

function normalizeSkill(skill: SkillInfoFromBackend, previous?: SkillInfo): SkillInfo {
  return {
    ...skill,
    displayName: skill.displayName || skill.id,
    displayNameEn: skill.displayNameEn || skill.displayName || skill.id,
    description: skill.description || '',
    enabled: skill.enabled ?? previous?.enabled ?? true,
    icon: skill.icon || '',
    shortDescription: skill.shortDescription || skill.description || '',
    shortDescriptionEn: skill.shortDescriptionEn || skill.displayNameEn || skill.displayName || '',
    triggerText: skill.triggerText || `/${skill.id}`,
    category: skill.category || 'general',
    updatedAt: skill.updatedAt ?? null,
  }
}

interface SkillState {
  skills: SkillInfo[]
  recommendedIds: string[]
  isLoading: boolean
  listByCategory: (id: SkillCategoryId) => SkillInfo[]
  getById: (id: string) => SkillInfo | null
  reload: () => Promise<void>
  install: (id: string) => Promise<void>
  uninstall: (id: string) => Promise<void>
  upload: (sourcePath: string, force?: boolean) => Promise<void>
  setSkillEnabled: (skillId: string, enabled: boolean) => Promise<void>
  reset: () => void
}

export const useSkillStore = create<SkillState>((set, get) => ({
  skills: [],
  recommendedIds: RECOMMENDED_SKILL_IDS,
  isLoading: false,
  reset: () => set({ skills: [], isLoading: false }),
  listByCategory(id) {
    const { skills, recommendedIds } = get()
    if (id == 'recommended') {
      return skills.filter((skill) => recommendedIds.includes(skill.id))
    }
    return skills.filter((skill) => (skill.category || 'general') === id)
  },
  getById(id) {
    return get().skills.find((skill) => skill.id === id) ?? null
  },
  async reload() {
    set({ isLoading: true })
    try {
      const previousById = new Map(get().skills.map((skill) => [skill.id, skill]))
      const skills = (await listSkills()).map((skill) => normalizeSkill(skill, previousById.get(skill.id)))
      set({ skills, isLoading: false })
    } catch (error) {
      set({ isLoading: false })
      throw error
    }
  },
  async install() {
    throw new Error('技能市场即将开放')
  },
  async uninstall(id) {
    await uninstallCustomSkill(id)
    await get().reload()
  },
  async upload(sourcePath, force = false) {
    try {
      await installCustomSkill(sourcePath, force)
      await get().reload()
    } catch (err) {
      throw toInstallError(err)
    }
  },
  async setSkillEnabled(skillId, enabled) {
    const previousSkills = get().skills
    set({
      skills: previousSkills.map((skill) =>
        skill.id === skillId ? { ...skill, enabled } : skill,
      ),
    })
    try {
      await setSkillEnabledIpc(skillId, enabled)
    } catch (err) {
      set({ skills: previousSkills })
      throw err
    }
  },
}))
