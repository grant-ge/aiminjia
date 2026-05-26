import {
  BarChart2,
  Briefcase,
  Building2,
  Clipboard,
  Coins,
  FileSearch,
  FileText,
  Folder,
  Heart,
  PenLine,
  Scale,
  Scroll,
  ShoppingCart,
  Smartphone,
  Target,
  TrendingUp,
  Users,
  type LucideIcon,
} from 'lucide-react'

const ICONS: Record<string, LucideIcon> = {
  'bar-chart-2': BarChart2,
  briefcase: Briefcase,
  'building-2': Building2,
  clipboard: Clipboard,
  'clipboard-list': Clipboard,
  coins: Coins,
  'file-search': FileSearch,
  'file-text': FileText,
  folder: Folder,
  heart: Heart,
  'pen-line': PenLine,
  scale: Scale,
  scroll: Scroll,
  'shopping-cart': ShoppingCart,
  smartphone: Smartphone,
  target: Target,
  'trending-up': TrendingUp,
  users: Users,
}

const CATEGORY_BG: Record<string, string> = {
  hr: 'bg-blue-500',
  finance: 'bg-emerald-500',
  legal: 'bg-violet-500',
  sales: 'bg-orange-500',
  ops: 'bg-rose-500',
  general: 'bg-amber-500',
}

export function getSkillIconComponent(icon: string | null | undefined): LucideIcon {
  if (!icon) return FileText
  return ICONS[icon] ?? FileText
}

export function getSkillCategoryBg(category: string | null | undefined): string {
  if (!category) return 'bg-slate-500'
  return CATEGORY_BG[category] ?? 'bg-slate-500'
}
