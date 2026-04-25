import { create } from 'zustand'

import type { SkillCategoryId } from '@/data/skill-categories'
import { installCustomSkill, listSkills, uninstallCustomSkill, type SkillInfo } from '@/lib/tauri'

const RECOMMENDED_SKILL_IDS = ['skill-smith', 'salary-benchmarking', 'biz-writing', 'contract-review']

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
      const skills = await listSkills()
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
