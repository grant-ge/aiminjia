import {
  BarChart2,
  Bot,
  BrainCircuit,
  Briefcase,
  Building2,
  CalendarClock,
  Clipboard,
  Coins,
  Database,
  FileSearch,
  FileSpreadsheet,
  FileText,
  Folder,
  Globe,
  GraduationCap,
  Handshake,
  Heart,
  Mail,
  Megaphone,
  MessageSquare,
  PenLine,
  Scale,
  Search,
  Scroll,
  ShoppingCart,
  Sparkles,
  Smartphone,
  Settings,
  Store,
  Target,
  TrendingUp,
  User,
  Users,
  Wrench,
  type LucideIcon,
} from 'lucide-react'
import type { ReactNode } from 'react'

const ICONS: Record<string, LucideIcon> = {
  'bar-chart-2': BarChart2,
  bot: Bot,
  brain: BrainCircuit,
  briefcase: Briefcase,
  building: Building2,
  'building-2': Building2,
  calendar: CalendarClock,
  'calendar-clock': CalendarClock,
  clipboard: Clipboard,
  'clipboard-list': Clipboard,
  coins: Coins,
  database: Database,
  'file-search': FileSearch,
  'file-spreadsheet': FileSpreadsheet,
  'file-text': FileText,
  folder: Folder,
  globe: Globe,
  'graduation-cap': GraduationCap,
  handshake: Handshake,
  heart: Heart,
  mail: Mail,
  megaphone: Megaphone,
  'message-square': MessageSquare,
  'pen-line': PenLine,
  scale: Scale,
  search: Search,
  scroll: Scroll,
  'shopping-cart': ShoppingCart,
  sparkles: Sparkles,
  smartphone: Smartphone,
  settings: Settings,
  store: Store,
  target: Target,
  'trending-up': TrendingUp,
  user: User,
  users: Users,
  wrench: Wrench,
}

const CATEGORY_BG: Record<string, string> = {
  hr: 'bg-blue-500',
  finance: 'bg-emerald-500',
  legal: 'bg-violet-500',
  sales: 'bg-orange-500',
  ops: 'bg-rose-500',
  general: 'bg-amber-500',
}

const CATEGORY_AVATAR_CLASS: Record<string, string> = {
  hr: 'bg-[var(--color-semantic-blue-bg-light)] text-muted-foreground',
  finance: 'bg-[var(--color-semantic-green-bg-light)] text-muted-foreground',
  legal: 'bg-[var(--color-semantic-purple-bg-light)] text-muted-foreground',
  sales: 'bg-[var(--color-semantic-orange-bg-light)] text-muted-foreground',
  ops: 'bg-[var(--color-semantic-red-bg-light)] text-muted-foreground',
  general: 'bg-[var(--color-accent-bg-light)] text-muted-foreground',
}

export const SKILL_FALLBACK_AVATAR_CLASS = 'bg-[#fbeed8] text-[#d19b00]'

const SKILL_AVATARS_BY_ID: Record<string, string> = {
  'bid-writing': '/skill-avatars/bid-writing.jpg',
  'biz-writing': '/skill-avatars/biz-writing.jpg',
  'multi-file-handler': '/skill-avatars/biz-writing.jpg',
  'budget-analysis': '/skill-avatars/finance-yuan.jpg',
  'finance-analysis': '/skill-avatars/finance-yuan.jpg',
  'salary-benchmarking': '/skill-avatars/finance-yuan.jpg',
  'salary-query': '/skill-avatars/finance-yuan.jpg',
  'competitive-intelligence': '/skill-avatars/competitive-intelligence.jpg',
  'user-behavior': '/skill-avatars/competitive-intelligence.jpg',
  'okr-coach': '/skill-avatars/okr-coach.jpg',
  'org-diagnosis': '/skill-avatars/org-diagnosis.jpg',
  'talent-9box': '/skill-avatars/org-diagnosis.jpg',
  'engagement-survey': '/skill-avatars/engagement-survey.jpg',
  'survey-analysis': '/skill-avatars/engagement-survey.jpg',
  'recruitment-funnel': '/skill-avatars/recruitment-funnel.jpg',
  'customer-segmentation': '/skill-avatars/recruitment-funnel.jpg',
  'html-ppt': '/skill-avatars/html-ppt.jpg',
  'contract-review': '/skill-avatars/policy-compliance-audit.jpg',
  'policy-compliance-audit': '/skill-avatars/policy-compliance-audit.jpg',
  'ops-analysis': '/skill-avatars/ops-analysis.jpg',
  'sales-analysis': '/skill-avatars/finance-yuan.jpg',
  'sales-followup-rules': '/skill-avatars/recruitment-funnel.jpg',
  browser: '/skill-avatars/web-access.svg',
  payslip: '/skill-avatars/smart-payslip.jpg',
  smartcb: '/skill-avatars/smart-compensation.jpg',
  rehcm: '/skill-avatars/renlijia-hr.jpg',
  'dingtalk-workspace': '/logos/dingtalk.png',
  'xiaojia-doctor': '/brand-avatar-gold.svg',
}

export function getSkillIconComponent(icon: string | null | undefined): LucideIcon {
  if (!icon) return FileText
  return ICONS[icon] ?? FileText
}

export function getKnownSkillIconComponent(icon: string | null | undefined): LucideIcon | null {
  if (!icon) return null
  return ICONS[icon] ?? null
}

export function getSkillContentIconComponent(text: string): LucideIcon | null {
  const value = text.toLowerCase()
  if (/(薪酬|薪资|salary|compensation|pay|工资)/i.test(value)) return Coins
  if (/(财务|预算|finance|budget|成本|费用)/i.test(value)) return BarChart2
  if (/(合同|法务|合规|法律|contract|legal|compliance|audit)/i.test(value)) return Scale
  if (/(招聘|简历|候选人|recruit|resume|candidate|interview)/i.test(value)) return Users
  if (/(调研|问卷|survey|访谈|满意度)/i.test(value)) return Clipboard
  if (/(写作|文案|标书|proposal|writing|bid|draft)/i.test(value)) return PenLine
  if (/(数据|分析|报表|报告|analysis|analytics|report|dashboard)/i.test(value)) return BarChart2
  if (/(浏览器|网页|browser|web)/i.test(value)) return Globe
  if (/(文件|文档|file|document|docx|pdf|excel|csv)/i.test(value)) return FileText
  if (/(客户|销售|商机|sales|customer|crm)/i.test(value)) return TrendingUp
  if (/(okr|目标|绩效|performance)/i.test(value)) return Target
  if (/(组织|架构|岗位|org|organization|position)/i.test(value)) return Building2
  if (/(自动化|流程|workflow|automation|bot)/i.test(value)) return Bot
  return null
}

export function getSkillCategoryBg(category: string | null | undefined): string {
  if (!category) return 'bg-slate-500'
  return CATEGORY_BG[category] ?? 'bg-slate-500'
}

export function getSkillAvatarClass(category: string | null | undefined): string {
  if (!category) return 'bg-muted text-muted-foreground'
  return CATEGORY_AVATAR_CLASS[category] ?? 'bg-muted text-muted-foreground'
}

export function getSkillAvatarSrc(skillId: string | null | undefined): string | null {
  if (!skillId) return null
  return SKILL_AVATARS_BY_ID[skillId] ?? null
}

export function getSkillAvatarNode(skillId: string | null | undefined): ReactNode | null {
  const src = getSkillAvatarSrc(skillId)
  if (!src) return null
  return (
    <img
      src={src}
      alt=""
      draggable={false}
      className="h-full w-full rounded-md object-cover"
    />
  )
}

export function getSkillCardAvatarClass(skillId: string | null | undefined): string {
  return getSkillAvatarSrc(skillId) ? 'bg-transparent' : SKILL_FALLBACK_AVATAR_CLASS
}
