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
import { useEffect, useMemo, useState } from 'react'

import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { SkillCard } from '@/components/skills/SkillCard'
import { SkillCategoryBar } from '@/components/skills/SkillCategoryBar'
import { SkillHotSection } from '@/components/skills/SkillHotSection'
import { SkillOfficeSection } from '@/components/skills/SkillOfficeSection'
import { Button } from '@/components/ui/button'
import { SKILL_CATEGORIES, type SkillCategoryId } from '@/data/skill-categories'
import { useChat } from '@/hooks/useChat'
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

const CATEGORY_STYLE: Record<string, { bg: string }> = {
  hr:      { bg: 'bg-blue-500' },
  finance: { bg: 'bg-emerald-500' },
  legal:   { bg: 'bg-violet-500' },
  sales:   { bg: 'bg-orange-500' },
  ops:     { bg: 'bg-rose-500' },
  general: { bg: 'bg-amber-500' },
}

function getSkillIcon(icon: string) {
  const Icon = ICONS[icon] ?? FileText
  return <Icon className="h-4 w-4 text-white" />
}

function getIconBg(category: string) {
  return CATEGORY_STYLE[category]?.bg ?? 'bg-slate-500'
}

export function SkillCenterPage() {
  const [category, setCategory] = useState<SkillCategoryId>('recommended')
  const [query, setQuery] = useState('')
  const [uploadOpen, setUploadOpen] = useState(false)
  const [loadError, setLoadError] = useState<string | null>(null)
  const skills = useSkillStore((s) => s.skills)
  const isLoading = useSkillStore((s) => s.isLoading)
  const reload = useSkillStore((s) => s.reload)
  const listByCategory = useSkillStore((s) => s.listByCategory)
  const setRoute = useUiStore((s) => s.setRoute)
  const { createConversationFromSkill } = useChat()

  const loadSkills = async () => {
    setLoadError(null)
    try {
      await reload()
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      setLoadError(message)
      console.error('Failed to load skills:', error)
    }
  }

  useEffect(() => {
    void loadSkills()
  }, [reload])

  const categoryItems = useMemo(
    () => [
      { key: 'recommended', label: '全部' },
      ...SKILL_CATEGORIES.map((c) => ({ key: c.id, label: c.name })),
    ],
    [],
  )

  const normalizedQuery = query.trim().toLowerCase()
  const matchesQuery = (skill: (typeof skills)[number]) => {
    if (!normalizedQuery) return true
    return [
      skill.displayName,
      skill.displayNameEn,
      skill.description,
      skill.shortDescription,
      skill.shortDescriptionEn,
      skill.triggerText,
      skill.category,
    ].some((value) => value?.toLowerCase().includes(normalizedQuery))
  }

  const recommended = listByCategory('recommended').filter(matchesQuery)
  const recommendedIdSet = new Set(recommended.map((s) => s.id))
  const officeSkills =
    category === 'recommended'
      ? skills.filter((s) => !recommendedIdSet.has(s.id)).filter(matchesQuery)
      : listByCategory(category).filter(matchesQuery)

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
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                className="flex-1 bg-transparent text-[13px] text-foreground outline-none placeholder:text-muted-foreground"
                placeholder="搜索技能名称或场景"
              />
            </div>
            <Button variant="outline" onClick={() => setUploadOpen(true)}>
              上传技能资料
            </Button>
            <Button onClick={() => setUploadOpen(true)}>
              + 导入技能
            </Button>
          </div>
        </header>
      }
      padding="px-7 pt-6 pb-8"
      gap="gap-5"
    >
      <SkillHotSection>
        {isLoading && skills.length === 0 ? (
          <SkillCenterState title="正在加载技能..." />
        ) : loadError && skills.length === 0 ? (
          <SkillCenterState title="技能加载失败" desc={loadError} actionLabel="重试" onAction={() => void loadSkills()} />
        ) : recommended.length === 0 && officeSkills.length === 0 ? (
          <SkillCenterState title="还没有可用技能" desc="可以上传本地技能目录，或点击创建技能开始制作。" actionLabel="重新加载" onAction={() => void loadSkills()} />
        ) : recommended.slice(0, 4).map((skill) => (
          <SkillCard
            key={skill.id}
            size="hot"
            title={skill.displayName}
            meta={getSkillMeta(skill.source, skill.category)}
            desc={skill.shortDescription || skill.description}
            iconNode={getSkillIcon(skill.icon)}
            iconBg={getIconBg(skill.category)}
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
        {!isLoading && !loadError && officeSkills.map((skill) => (
          <SkillCard
            key={skill.id}
            title={skill.displayName}
            meta={getSkillMeta(skill.source, skill.category)}
            desc={skill.shortDescription || skill.description}
            iconNode={getSkillIcon(skill.icon)}
            iconBg={getIconBg(skill.category)}
            onClick={() => setRoute({ kind: 'skill-detail', skillId: skill.id })}
          />
        ))}
      </SkillOfficeSection>
      <SkillUploadModal open={uploadOpen} onOpenChange={setUploadOpen} />
    </PageSectionShell>
  )
}

function SkillCenterState({
  title,
  desc,
  actionLabel,
  onAction,
}: {
  title: string
  desc?: string
  actionLabel?: string
  onAction?: () => void
}) {
  return (
    <div className="col-span-full rounded-[14px] border border-dashed border-border bg-card/60 p-6 text-sm">
      <div className="font-semibold text-foreground">{title}</div>
      {desc ? <p className="mt-1 text-muted-foreground">{desc}</p> : null}
      {actionLabel && onAction ? (
        <Button className="mt-3" variant="outline" size="sm" onClick={onAction}>
          {actionLabel}
        </Button>
      ) : null}
    </div>
  )
}
