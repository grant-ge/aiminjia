import { create } from 'zustand'

import type { SkillCategoryId } from '@/data/skill-categories'
import { installCustomSkill, listSkills, uninstallCustomSkill, type SkillInfo } from '@/lib/tauri'

const RECOMMENDED_SKILL_IDS = ['skill-smith', 'salary-benchmarking', 'biz-writing', 'contract-review']

function normalizeSkill(skill: SkillInfo): SkillInfo {
  return {
    ...skill,
    displayName: skill.displayName || skill.id,
    displayNameEn: skill.displayNameEn || skill.displayName || skill.id,
    description: skill.description || '',
    icon: skill.icon || '',
    shortDescription: skill.shortDescription || skill.description || '',
    shortDescriptionEn: skill.shortDescriptionEn || skill.displayNameEn || skill.displayName || '',
    triggerText: skill.triggerText || `/${skill.id}`,
    category: skill.category || 'general',
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
  upload: (sourcePath: string) => Promise<void>
}

export const useSkillStore = create<SkillState>((set, get) => ({
  skills: [],
  recommendedIds: RECOMMENDED_SKILL_IDS,
  isLoading: false,
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
      const skills = (await listSkills()).map(normalizeSkill)
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
  async upload(sourcePath) {
    await installCustomSkill(sourcePath)
    await get().reload()
  },
}))
