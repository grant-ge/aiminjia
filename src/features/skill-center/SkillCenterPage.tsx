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
  MoreHorizontal,
  PenLine,
  Scale,
  Scroll,
  Search,
  ShoppingCart,
  Smartphone,
  Target,
  Trash2,
  TrendingUp,
  Users,
  type LucideIcon,
} from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'

import { AppDropdown } from '@/components/common/AppDropdown'
import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { SkillCard } from '@/components/skills/SkillCard'
import { SkillCategoryBar } from '@/components/skills/SkillCategoryBar'
import { SkillOfficeSection } from '@/components/skills/SkillOfficeSection'
import { Button } from '@/components/ui/button'
import { SKILL_CATEGORIES, type SkillCategoryId } from '@/data/skill-categories'
import { useChat } from '@/hooks/useChat'
import { useNotificationStore } from '@/stores/notificationStore'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

import { SkillUploadModal } from './SkillUploadModal'
import { SkillDraftBanner } from './SkillDraftBanner'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { importSkillPackagesWithUI } from '@/hooks/useDragDropListener'

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
  return <Icon className="h-4 w-4 text-primary-foreground" />
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
  const uninstall = useSkillStore((s) => s.uninstall)
  const listByCategory = useSkillStore((s) => s.listByCategory)
  const setRoute = useUiStore((s) => s.setRoute)
  const pushNotification = useNotificationStore((s) => s.push)
  useChat()

  const handleImportPackage = useCallback(async () => {
    const picked = await openDialog({
      multiple: true,
      filters: [{ name: 'AIjia Skill Package', extensions: ['aijia-skill'] }],
    })
    if (!picked) return
    const paths = Array.isArray(picked) ? picked : [picked]
    if (paths.length === 0) return
    await importSkillPackagesWithUI(paths)
    void reload()
  }, [reload])

  const handleDeleteSkill = async (skillId: string, displayName: string) => {
    const confirmed = await requestConfirm({
      title: '删除技能',
      description: `确定要删除「${displayName}」吗？此操作不可撤销。`,
      confirmLabel: '删除',
      cancelLabel: '取消',
      variant: 'destructive',
    })
    if (!confirmed) return
    try {
      await uninstall(skillId)
      pushNotification({
        level: 'success',
        title: '技能已删除',
        message: `「${displayName}」已从技能中心移除。`,
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      pushNotification({
        level: 'error',
        title: '删除技能失败',
        message,
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
    }
  }

  const handleExportSkill = async (skillId: string, displayName: string) => {
    try {
      const dest = await invoke<string>('export_installed_skill', { skillId })
      pushNotification({
        level: 'success',
        title: '技能已导出',
        message: `「${displayName}」已保存到 ${dest}。把这个 .aijia-skill 文件发给同事，对方双击即可安装。`,
        actions: [],
        dismissible: true,
        autoHide: 10,
        context: 'toast',
      })
    } catch (err) {
      pushNotification({
        level: 'error',
        title: '导出失败',
        message: String(err),
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
    }
  }

  const handleLoadError = (error: unknown) => {
    const message = error instanceof Error ? error.message : String(error)
    setLoadError(message)
    console.error('Failed to load skills:', error)
  }

  const loadSkills = useCallback(async () => {
    setLoadError(null)
    try {
      await reload()
    } catch (error) {
      handleLoadError(error)
    }
  }, [reload])

  useEffect(() => {
    void reload().catch(handleLoadError)
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

  const officeSkills =
    category === 'recommended'
      ? skills.filter(matchesQuery)
      : category === 'mine'
        ? skills.filter((s) => s.source === 'user').filter(matchesQuery)
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
        <header data-tauri-drag-region className="flex h-[45px] items-center justify-between border-b border-border px-6">
          <div className="flex items-center gap-3">
            <span className="text-base font-semibold text-foreground">技能中心</span>
            <span className="rounded-full bg-secondary px-2 py-0.5 text-[0.6875rem] font-medium text-muted-foreground">
              已安装 {skills.length} 个技能
            </span>
          </div>
          <div className="flex items-center gap-2">
            <div className="flex h-7 w-[180px] items-center gap-1.5 rounded-full bg-secondary px-2.5">
              <Search className="h-3 w-3 shrink-0 text-muted-foreground" />
              <input
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                className="flex-1 bg-transparent text-xs text-foreground outline-none placeholder:text-muted-foreground"
                placeholder="搜索技能名称或场景"
              />
            </div>
            <Button size="sm" variant="outline" onClick={() => void handleImportPackage()}>
              + 导入 .aijia-skill
            </Button>
            <Button size="sm" onClick={() => setUploadOpen(true)}>
              + 导入技能
            </Button>
          </div>
        </header>
      }
      padding="px-7 pt-6 pb-8"
      gap="gap-5"
    >
      <SkillDraftBanner />
      <SkillOfficeSection
        categoryBar={
          <SkillCategoryBar
            items={categoryItems}
            activeKey={category}
            onSelect={(key) => setCategory(key as SkillCategoryId)}
          />
        }
      >
        {isLoading && skills.length === 0 ? (
          <SkillCenterState title="正在加载技能..." />
        ) : loadError && skills.length === 0 ? (
          <SkillCenterState title="技能加载失败" desc={loadError} actionLabel="重试" onAction={() => void loadSkills()} />
        ) : officeSkills.length === 0 ? (
          <SkillCenterState title="还没有可用技能" desc="可以上传本地技能目录，或点击创建技能开始制作。" actionLabel="重新加载" onAction={() => void loadSkills()} />
        ) : (
          officeSkills.map((skill) => (
            <SkillCard
              key={skill.id}
              title={skill.displayName}
              meta={getSkillMeta(skill.source, skill.category)}
              desc={skill.shortDescription || skill.description}
              iconNode={getSkillIcon(skill.icon)}
              iconBg={getIconBg(skill.category)}
              onClick={() => setRoute({ kind: 'skill-detail', skillId: skill.id })}
              actionsSlot={skill.source !== 'user' ? (
                <div aria-hidden="true" className="h-7 w-7" />
              ) : (
                <AppDropdown
                  ariaLabel={`${skill.displayName} 更多操作`}
                  trigger={
                    <button
                      type="button"
                      className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground"
                    >
                      <MoreHorizontal className="h-4 w-4" />
                    </button>
                  }
                  items={[
                    {
                      id: 'export',
                      label: '导出 .aijia-skill',
                      onSelect: () => void handleExportSkill(skill.id, skill.displayName),
                    },
                    {
                      id: 'delete',
                      label: '删除技能',
                      icon: <Trash2 />,
                      className: 'text-destructive [&_svg]:text-destructive',
                      onSelect: () => void handleDeleteSkill(skill.id, skill.displayName),
                    },
                  ]}
                />
              )}
            />
          ))
        )}
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
