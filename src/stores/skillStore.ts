import { create } from 'zustand'

import type { SkillCategoryId } from '@/data/skill-categories'
import { listSkills, type SkillInfo } from '@/lib/tauri'

const RECOMMENDED_SKILL_IDS = ['writing-plans', 'skill-smith', 'table-analysis', 'ppt-builder', 'research-brief']

interface SkillState {
  skills: SkillInfo[]
  recommendedIds: string[]
  isLoading: boolean
  listByCategory: (id: SkillCategoryId) => SkillInfo[]
  getById: (id: string) => SkillInfo | null
  reload: () => Promise<void>
  install: (id: string) => Promise<void>
  uninstall: (id: string) => Promise<void>
  upload: (file: File) => Promise<void>
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
  async uninstall() {
    throw new Error('卸载功能即将开放')
  },
  async upload() {
    throw new Error('上传功能即将开放')
  },
}))
