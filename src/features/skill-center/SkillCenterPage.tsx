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
  Search,
  ShoppingCart,
  Smartphone,
  Target,
  TrendingUp,
  Users,
  type LucideIcon,
} from 'lucide-react'
import { useMemo, useState } from 'react'

import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { SkillCard } from '@/components/skills/SkillCard'
import { SkillCategoryBar } from '@/components/skills/SkillCategoryBar'
import { SkillHotSection } from '@/components/skills/SkillHotSection'
import { SkillOfficeSection } from '@/components/skills/SkillOfficeSection'
import { Button } from '@/components/ui/button'
import { SKILL_CATEGORIES, type SkillCategoryId } from '@/data/skill-categories'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

import { SkillUploadModal } from './SkillUploadModal'

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

function getSkillIcon(icon: string) {
  const Icon = ICONS[icon] ?? FileText
  return <Icon className="h-4 w-4 text-primary" />
}

export function SkillCenterPage() {
  const [category, setCategory] = useState<SkillCategoryId>('recommended')
  const [uploadOpen, setUploadOpen] = useState(false)
  const skills = useSkillStore((s) => s.skills)
  const listByCategory = useSkillStore((s) => s.listByCategory)
  const setRoute = useUiStore((s) => s.setRoute)

  const categoryItems = useMemo(
    () => [
      { key: 'recommended', label: '全部' },
      ...SKILL_CATEGORIES.map((c) => ({ key: c.id, label: c.name })),
    ],
    [],
  )

  const recommended = listByCategory('recommended')
  const recommendedIdSet = new Set(recommended.map((s) => s.id))
  const officeSkills =
    category === 'recommended'
      ? skills.filter((s) => !recommendedIdSet.has(s.id))
      : listByCategory(category)

  function getSkillMeta(source: string, cat: string) {
    const normalizedCategory = cat || 'general'
    const label = SKILL_CATEGORIES.find((c) => c.id === normalizedCategory)?.name ?? '通用'
    const sourceLabel = source === 'builtin' ? '内置' : '自定义'
    return `${sourceLabel} · ${label}`
  }

  return (
    <PageSectionShell
      topBar={
        <header data-tauri-drag-region className="flex h-14 items-center justify-between border-b border-border px-6">
          <div className="flex items-center gap-3">
            <span className="text-[18px] font-bold text-foreground">技能中心</span>
            <span className="rounded-full bg-secondary px-2.5 py-1 text-[12px] font-medium text-muted-foreground">
              {skills.length} 个技能
            </span>
          </div>
          <div className="flex items-center gap-2.5">
            <div className="flex h-[34px] w-[220px] items-center gap-2 rounded-full bg-secondary px-3">
              <Search className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              <input
                className="flex-1 bg-transparent text-[13px] text-foreground outline-none placeholder:text-muted-foreground"
                placeholder="搜索技能名称或场景"
                readOnly
              />
            </div>
            <Button variant="outline" onClick={() => setUploadOpen(true)}>
              上传技能资料
            </Button>
            <Button>+ 创建技能</Button>
          </div>
        </header>
      }
      padding="px-7 pt-6 pb-8"
      gap="gap-5"
    >
      <SkillHotSection>
        {recommended.slice(0, 4).map((skill) => (
          <SkillCard
            key={skill.id}
            size="hot"
            title={skill.displayName}
            meta={getSkillMeta(skill.source, skill.category)}
            desc={skill.shortDescription || skill.description}
            iconNode={getSkillIcon(skill.icon)}
            onClick={() => setRoute({ kind: 'skill-detail', skillId: skill.id })}
          />
        ))}
        </SkillHotSection>
      <SkillOfficeSection
        categoryBar={
          <SkillCategoryBar
            items={categoryItems}
            activeKey={category}
            onSelect={(key) => setCategory(key as SkillCategoryId)}
          />
        }
      >
        {officeSkills.map((skill) => (
          <SkillCard
            key={skill.id}
            title={skill.displayName}
            meta={getSkillMeta(skill.source, skill.category)}
            desc={skill.shortDescription || skill.description}
            iconNode={getSkillIcon(skill.icon)}
            onClick={() => setRoute({ kind: 'skill-detail', skillId: skill.id })}
          />
        ))}
      </SkillOfficeSection>
      <SkillUploadModal open={uploadOpen} onOpenChange={setUploadOpen} />
    </PageSectionShell>
  )
}
