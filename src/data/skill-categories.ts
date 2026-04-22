export type SkillCategoryId =
  | 'recommended'
  | 'general'
  | 'ecommerce'
  | 'finance'
  | 'design'
  | 'dev'
  | 'legal'
  | 'media'
  | 'health'
  | 'ops'
  | 'content'

export interface SkillCategory {
  id: Exclude<SkillCategoryId, 'recommended'>
  name: string
  icon: string
}

export const SKILL_CATEGORIES: SkillCategory[] = [
  { id: 'general', name: '通用工具', icon: 'wrench' },
  { id: 'ecommerce', name: '电商', icon: 'shopping-cart' },
  { id: 'finance', name: '门店与财务', icon: 'store' },
  { id: 'design', name: '设计与制造', icon: 'pencil-ruler' },
  { id: 'dev', name: '开发', icon: 'code' },
  { id: 'legal', name: '律所', icon: 'scale' },
  { id: 'media', name: '媒介', icon: 'megaphone' },
  { id: 'health', name: '健康与学习', icon: 'heart-pulse' },
  { id: 'ops', name: '运营', icon: 'trending-up' },
  { id: 'content', name: '内容创作', icon: 'feather' },
]
