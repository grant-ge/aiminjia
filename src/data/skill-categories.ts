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
  { id: 'mine',    name: '本地',  icon: 'user' },
  { id: 'hr',      name: 'HR',   icon: 'users' },
  { id: 'finance', name: '财务', icon: 'bar-chart-2' },
  { id: 'legal',   name: '法务', icon: 'scale' },
  { id: 'sales',   name: '销售', icon: 'trending-up' },
  { id: 'ops',     name: '运营', icon: 'settings' },
  { id: 'general', name: '通用', icon: 'wrench' },
]
