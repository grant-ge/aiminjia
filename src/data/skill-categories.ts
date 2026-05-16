import i18n from '@/i18n'

export type SkillCategoryId =
  | 'recommended'
  | 'mine'
  | 'hr'
  | 'finance'
  | 'legal'
  | 'sales'
  | 'ops'
  | 'general'

export interface SkillCategory {
  id: Exclude<SkillCategoryId, 'recommended'>
  name: string
  icon: string
}

export const SKILL_CATEGORIES: SkillCategory[] = [
  { id: 'mine',    get name() { return i18n.t('skillCategories.mine') },  icon: 'user' },
  { id: 'hr',      name: 'HR',   icon: 'users' },
  { id: 'finance', get name() { return i18n.t('skillCategories.finance') }, icon: 'bar-chart-2' },
  { id: 'legal',   get name() { return i18n.t('skillCategories.legal') }, icon: 'scale' },
  { id: 'sales',   get name() { return i18n.t('skillCategories.sales') }, icon: 'trending-up' },
  { id: 'ops',     get name() { return i18n.t('skillCategories.ops') }, icon: 'settings' },
  { id: 'general', get name() { return i18n.t('skillCategories.general') }, icon: 'wrench' },
]
