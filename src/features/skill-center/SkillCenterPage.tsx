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
import { syncBuiltinSkills } from '@/lib/tauri'
import { useAuthStore } from '@/stores/authStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

import { SkillDraftBanner } from './SkillDraftBanner'
import { SkillValidationResultDialog } from './SkillValidationResultDialog'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { SkillValidationError, type SkillValidationKind } from '@/stores/skillStore'
import { uploadWithOverwriteConfirm } from './uploadWithOverwriteConfirm'

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
  const [loadError, setLoadError] = useState<string | null>(null)
  const [validationFailure, setValidationFailure] = useState<
    { kind: SkillValidationKind; detail?: string } | null
  >(null)
  const [syncing, setSyncing] = useState(false)
  const [checkingId, setCheckingId] = useState<string | null>(null)
  const skills = useSkillStore((s) => s.skills)
  const isLoading = useSkillStore((s) => s.isLoading)
  const reload = useSkillStore((s) => s.reload)
  const upload = useSkillStore((s) => s.upload)
  const uninstall = useSkillStore((s) => s.uninstall)
  const listByCategory = useSkillStore((s) => s.listByCategory)
  const setRoute = useUiStore((s) => s.setRoute)
  const pushNotification = useNotificationStore((s) => s.push)
  const isLoggedIn = useAuthStore((s) => s.isLoggedIn)
  useChat()

  const handleImportDirectory = useCallback(async () => {
    const picked = await openDialog({
      directory: true,
      multiple: false,
      title: '选择技能目录',
    })
    if (!picked || Array.isArray(picked)) return

    try {
      const outcome = await uploadWithOverwriteConfirm((force) => upload(picked, force))
      if (outcome === 'installed') {
        pushNotification({
          level: 'success',
          title: '技能上传成功',
          message: '技能已安装到本地并刷新到技能中心。',
          actions: [],
          dismissible: true,
          autoHide: 4,
          context: 'toast',
        })
      }
    } catch (err) {
      if (err instanceof SkillValidationError) {
        setValidationFailure({ kind: err.kind, detail: err.detail })
        return
      }
      pushNotification({
        level: 'error',
        title: '技能上传失败',
        message: err instanceof Error ? err.message : String(err),
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
    }
  }, [pushNotification, upload])

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

  const handleSyncBuiltin = async () => {
    if (syncing) return
    setSyncing(true)
    try {
      const result = await syncBuiltinSkills()
      await reload()
      pushNotification({
        level: 'success',
        title:
          result.installed.length > 0
            ? `同步完成，更新 ${result.installed.length} 个技能`
            : '已是最新',
        message: '',
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } catch (err) {
      pushNotification({
        level: 'error',
        title: '同步失败',
        message: err instanceof Error ? err.message : String(err),
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
    } finally {
      setSyncing(false)
    }
  }

  /**
   * Per-card check-update: reuses the global sync IPC and inspects the
   * `installed` array to surface a card-targeted toast. Avoids a separate
   * per-skill backend command — the OPS list API has no per-skill query
   * so a dedicated command would still fetch the whole list.
   */
  const handleCheckSkillUpdate = async (skillId: string, displayName: string) => {
    if (checkingId || syncing) return
    setCheckingId(skillId)
    try {
      const result = await syncBuiltinSkills()
      await reload()
      const updated = result.installed.includes(skillId)
      pushNotification({
        level: 'success',
        title: updated ? '有新版本' : '已是最新',
        message: displayName,
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } catch (err) {
      pushNotification({
        level: 'error',
        title: '检查更新失败',
        message: err instanceof Error ? err.message : String(err),
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
    } finally {
      setCheckingId(null)
    }
  }

  const handleExportSkill = async (skillId: string, displayName: string) => {
    try {
      const dest = await invoke<string>('export_installed_skill', { skillId })
      pushNotification({
        level: 'success',
        title: '技能已导出',
        message: `「${displayName}」已保存到 ${dest}。把这个压缩包发给同事，对方在「技能中心」导入即可。`,
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
    <>
    <PageSectionShell
      topBar={
        <header data-tauri-drag-region className="flex h-[45px] items-center justify-between border-b border-border px-6">
          <div className="flex items-center gap-3">
            <span className="text-base font-semibold text-foreground">技能中心</span>
            <span className="rounded-full bg-secondary px-2 py-0.5 text-xs font-medium text-muted-foreground">
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
            {isLoggedIn && (
              <Button
                size="sm"
                variant="outline"
                onClick={() => void handleSyncBuiltin()}
                disabled={syncing}
                data-testid="skills-sync-builtin"
              >
                {syncing ? '同步中...' : '同步内置技能'}
              </Button>
            )}
            <Button size="sm" onClick={() => void handleImportDirectory()}>
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
          category === 'mine' ? (
            <SkillCenterState
              title="还没有本地技能"
              desc='在对话里找数字员工"小程"聊聊，让它帮你把工作流程沉淀成可复用技能；或点右上角"+ 导入技能"选一个本地技能目录上传。'
            />
          ) : normalizedQuery ? (
            <SkillCenterState
              title="没有匹配的技能"
              desc={`"${normalizedQuery}" 下没找到技能。试试切到"全部"或换个关键词。`}
            />
          ) : (
            <SkillCenterState
              title="该分类下还没有技能"
              desc='试试"全部"或"本地"分类；或者点右上角"+ 导入技能"选一个本地技能目录上传。'
            />
          )
        ) : (
          officeSkills.map((skill) => {
            const isUserSkill = skill.source === 'user'
            const menuItems: Array<{
              id: string
              label: string
              icon?: React.ReactNode
              className?: string
              disabled?: boolean
              onSelect: () => void
            }> = []
            if (isUserSkill) {
              menuItems.push({
                id: 'export',
                label: '导出',
                onSelect: () => void handleExportSkill(skill.id, skill.displayName),
              })
              menuItems.push({
                id: 'delete',
                label: '删除技能',
                icon: <Trash2 />,
                className: 'text-destructive [&_svg]:text-destructive',
                onSelect: () => void handleDeleteSkill(skill.id, skill.displayName),
              })
            } else if (isLoggedIn) {
              // Non-user skills (builtin / global) can be re-synced from OPS.
              menuItems.push({
                id: 'check-update',
                label: checkingId === skill.id ? '检查中...' : '检查更新',
                disabled: checkingId === skill.id || syncing,
                onSelect: () =>
                  void handleCheckSkillUpdate(skill.id, skill.displayName),
              })
            }
            return (
              <SkillCard
                key={skill.id}
                title={skill.displayName}
                meta={getSkillMeta(skill.source, skill.category)}
                desc={skill.shortDescription || skill.description}
                iconNode={getSkillIcon(skill.icon)}
                iconBg={getIconBg(skill.category)}
                version={skill.version}
                onClick={() => setRoute({ kind: 'skill-detail', skillId: skill.id })}
                actionsSlot={
                  menuItems.length === 0 ? (
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
                      items={menuItems}
                    />
                  )
                }
              />
            )
          })
        )}
      </SkillOfficeSection>
    </PageSectionShell>
    <SkillValidationResultDialog
      open={validationFailure !== null}
      onOpenChange={(next) => {
        if (!next) setValidationFailure(null)
      }}
      failure={validationFailure}
      onRetry={() => {
        setValidationFailure(null)
        void handleImportDirectory()
      }}
    />
    </>
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
    <div className="col-span-full rounded-lg border border-dashed border-border bg-card/60 p-6 text-sm">
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
