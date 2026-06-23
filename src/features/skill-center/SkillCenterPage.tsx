import { Check, Download, MessageSquare, MoreHorizontal, Plus, Search, Trash2 } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'

import { AppDropdown } from '@/components/common/AppDropdown'
import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import { Switch } from '@/components/common/Switch'
import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { SkillCard } from '@/components/skills/SkillCard'
import { SkillCategoryBar } from '@/components/skills/SkillCategoryBar'
import { SkillOfficeSection } from '@/components/skills/SkillOfficeSection'
import {
  getSkillAvatarNode,
  getSkillCardAvatarClass,
} from '@/components/skills/skillVisual'
import { Button } from '@/components/ui/button'
import { SKILL_CATEGORIES } from '@/data/skill-categories'
import { useChat } from '@/hooks/useChat'
import {
  canToggleSkillEnablement,
  isBuiltinSkill,
  isSkillEnabled,
  skillMatchesCenterView,
  type SkillCenterView,
} from '@/lib/skillAvailability'
import { listMarketplaceSkills, refreshSkillRegistry, syncBuiltinSkills, type MarketplaceSkillItem, type SkillInfo } from '@/lib/tauri'
import { localizeSkill } from '@/lib/skillLocalization'
import { useAuthStore } from '@/stores/authStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

import { SkillValidationResultDialog } from './SkillValidationResultDialog'
import { SkillDetailDialog } from '@/features/skill-detail/SkillDetailDialog'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { SkillValidationError, type SkillValidationKind } from '@/stores/skillStore'
import { uploadWithOverwriteConfirm } from './uploadWithOverwriteConfirm'
import { ChevronDown, FolderOpen, Package } from 'lucide-react'

function getSkillCardIconBg(skillId: string | null | undefined) {
  return getSkillCardAvatarClass(skillId)
}

function compareVersionText(a?: string | null, b?: string | null) {
  const left = (a || '').trim()
  const right = (b || '').trim()
  if (left === right) return 0
  if (!left) return -1
  if (!right) return 1

  const leftParts = left.split(/[._-]/)
  const rightParts = right.split(/[._-]/)
  const len = Math.max(leftParts.length, rightParts.length)
  for (let i = 0; i < len; i += 1) {
    const lp = leftParts[i] ?? '0'
    const rp = rightParts[i] ?? '0'
    const ln = Number(lp)
    const rn = Number(rp)
    if (Number.isFinite(ln) && Number.isFinite(rn) && ln !== rn) {
      return ln - rn
    }
    if (lp !== rp) {
      return lp.localeCompare(rp)
    }
  }
  return 0
}

function marketScopeRank(item: MarketplaceSkillItem) {
  return item.scope === 'tenant' ? 2 : 1
}

function shouldReplaceMarketItem(current: MarketplaceSkillItem, candidate: MarketplaceSkillItem) {
  const scopeDelta = marketScopeRank(candidate) - marketScopeRank(current)
  if (scopeDelta !== 0) return scopeDelta > 0

  const versionDelta = compareVersionText(candidate.version, current.version)
  if (versionDelta !== 0) return versionDelta > 0

  return candidate.id > current.id
}

function dedupeMarketItems(items: MarketplaceSkillItem[]) {
  const byPluginId = new Map<string, MarketplaceSkillItem>()
  for (const item of items) {
    const current = byPluginId.get(item.pluginId)
    if (!current || shouldReplaceMarketItem(current, item)) {
      byPluginId.set(item.pluginId, item)
    }
  }
  return Array.from(byPluginId.values())
}

export function SkillCenterPage() {
  const { t, i18n } = useTranslation()
  const [view, setView] = useState<SkillCenterView>('market')
  const [query, setQuery] = useState('')
  const [loadError, setLoadError] = useState<string | null>(null)
  const [validationFailure, setValidationFailure] = useState<
    { kind: SkillValidationKind; detail?: string } | null
  >(null)
  const [marketItems, setMarketItems] = useState<MarketplaceSkillItem[]>([])
  const [marketLoading, setMarketLoading] = useState(false)
  const [marketError, setMarketError] = useState<string | null>(null)
  const [selectedMarketItem, setSelectedMarketItem] = useState<MarketplaceSkillItem | null>(null)
  const [activeInstalledSkillId, setActiveInstalledSkillId] = useState<string | null>(null)
  const [installingMarketId, setInstallingMarketId] = useState<string | null>(null)
  const [syncing, setSyncing] = useState(false)
  const skills = useSkillStore((s) => s.skills)
  const isLoading = useSkillStore((s) => s.isLoading)
  const reload = useSkillStore((s) => s.reload)
  const upload = useSkillStore((s) => s.upload)
  const installMarketplace = useSkillStore((s) => s.installMarketplace)
  const uninstall = useSkillStore((s) => s.uninstall)
  const setSkillEnabled = useSkillStore((s) => s.setSkillEnabled)
  const setRoute = useUiStore((s) => s.setRoute)
  const setPendingSkill = useUiStore((s) => s.setPendingSkill)
  const pushNotification = useNotificationStore((s) => s.push)
  const isLoggedIn = useAuthStore((s) => s.isLoggedIn)
  const [enablementChangingId, setEnablementChangingId] = useState<string | null>(null)
  useChat()

  const runInstall = useCallback(
    async (picked: string) => {
      try {
        const outcome = await uploadWithOverwriteConfirm((force) => upload(picked, force))
        if (outcome === 'installed') {
          pushNotification({
            level: 'success',
            title: t('skillCenter.uploadSuccess'),
            message: t('skillCenter.uploadSuccessDesc'),
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
          title: t('skillCenter.uploadFailed'),
          message: err instanceof Error ? err.message : String(err),
          actions: [],
          dismissible: true,
          autoHide: 6,
          context: 'toast',
        })
      }
    },
    [pushNotification, t, upload],
  )

  const handleImportDirectory = useCallback(async () => {
    if (import.meta.env.DEV) {
      const queue = (window as unknown as { __aijia?: { _pickSkillImportMockQueue?: string[] } }).__aijia?._pickSkillImportMockQueue
      if (queue && queue.length > 0) {
        const mocked = queue.shift()
        if (mocked) {
          await runInstall(mocked)
          return
        }
      }
    }
    const picked = await openDialog({
      directory: true,
      multiple: false,
      title: t('skillCenter.selectDir'),
    })
    if (!picked || Array.isArray(picked)) return
    await runInstall(picked)
  }, [runInstall, t])

  const handleImportArchive = useCallback(async () => {
    if (import.meta.env.DEV) {
      const queue = (window as unknown as { __aijia?: { _pickSkillImportMockQueue?: string[] } }).__aijia?._pickSkillImportMockQueue
      if (queue && queue.length > 0) {
        const mocked = queue.shift()
        if (mocked) {
          await runInstall(mocked)
          return
        }
      }
    }
    const picked = await openDialog({
      directory: false,
      multiple: false,
      title: t('skillCenter.selectArchive'),
      filters: [
        { name: t('skillCenter.archiveFilter'), extensions: ['zip'] },
      ],
    })
    if (!picked || Array.isArray(picked)) return
    await runInstall(picked)
  }, [runInstall, t])

  const handleDeleteSkill = async (skillId: string, displayName: string, title = t('skillCenter.deleteSkill')) => {
    const confirmed = await requestConfirm({
      title,
      description: t('skillCenter.deleteConfirm', { name: displayName }),
      confirmLabel: t('skillCenter.deleteLabel'),
      cancelLabel: t('skillCenter.cancelLabel'),
      variant: 'destructive',
    })
    if (!confirmed) return
    try {
      await uninstall(skillId)
      pushNotification({
        level: 'success',
        title: t('skillCenter.skillDeleted'),
        message: t('skillCenter.skillDeletedDesc', { name: displayName }),
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      pushNotification({
        level: 'error',
        title: t('skillCenter.deleteFailed'),
        message,
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
    }
  }

  const handleSyncAll = async () => {
    if (syncing) return
    setSyncing(true)
    try {
      const result = await syncBuiltinSkills()
      await refreshSkillRegistry()
      await reload()
      if (isLoggedIn) {
        await loadMarketSkills()
      }
      pushNotification({
        level: 'success',
        title:
          result.installed.length > 0
            ? t('skillCenter.syncDone', { count: result.installed.length })
            : t('skillCenter.syncUpToDate'),
        message: '',
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } catch (err) {
      pushNotification({
        level: 'error',
        title: t('skillCenter.syncFailed'),
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

  const handleExportSkill = async (skillId: string, displayName: string) => {
    try {
      const dest = await invoke<string>('export_installed_skill', { skillId })
      pushNotification({
        level: 'success',
        title: t('skillCenter.exported'),
        message: t('skillCenter.exportedDesc', { name: displayName, dest }),
        actions: [],
        dismissible: true,
        autoHide: 10,
        context: 'toast',
      })
    } catch (err) {
      pushNotification({
        level: 'error',
        title: t('skillCenter.exportFailed'),
        message: String(err),
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
    }
  }

  const handleUseSkill = useCallback(
    (skill: SkillInfo) => {
      const localized = localizeSkill(skill, i18n.language)
      setPendingSkill({
        id: skill.id,
        label: localized.name,
        trigger: skill.triggerText?.trim() || `/${skill.id}`,
      })
      setRoute({ kind: 'home' })
    },
    [i18n.language, setPendingSkill, setRoute],
  )

  const loadMarketSkills = useCallback(async () => {
    if (!isLoggedIn) {
      setMarketItems([])
      setMarketError(null)
      return
    }
    setMarketLoading(true)
    setMarketError(null)
    try {
      const result = await listMarketplaceSkills(1, 100, undefined, query.trim() || undefined)
      setMarketItems(result.items)
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      setMarketError(message)
      setMarketItems([])
    } finally {
      setMarketLoading(false)
    }
  }, [isLoggedIn, query])

  const handleInstallMarketplace = useCallback(
    async (item: MarketplaceSkillItem) => {
      if (installingMarketId) return
      setInstallingMarketId(item.pluginId)
      try {
        await installMarketplace(item.id, item.pluginId)
      } catch (err) {
        pushNotification({
          level: 'error',
          title: '添加技能失败',
          message: err instanceof Error ? err.message : String(err),
          actions: [],
          dismissible: true,
          autoHide: 6,
          context: 'toast',
        })
      } finally {
        setInstallingMarketId(null)
      }
    },
    [installMarketplace, installingMarketId, pushNotification],
  )

  const handleSetSkillEnabled = useCallback(
    async (skill: SkillInfo, enabled: boolean) => {
      if (!canToggleSkillEnablement(skill)) return
      setEnablementChangingId(skill.id)
      try {
        await setSkillEnabled(skill.id, enabled)
        pushNotification({
          level: 'success',
          title: enabled ? '技能已开启' : '技能已关闭',
          message: localizeSkill(skill, i18n.language).name,
          actions: [],
          dismissible: true,
          autoHide: 3,
          context: 'toast',
        })
      } catch (err) {
        pushNotification({
          level: 'error',
          title: enabled ? '开启技能失败' : '关闭技能失败',
          message: err instanceof Error ? err.message : String(err),
          actions: [],
          dismissible: true,
          autoHide: 6,
          context: 'toast',
        })
      } finally {
        setEnablementChangingId(null)
      }
    },
    [i18n.language, pushNotification, setSkillEnabled],
  )

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

  useEffect(() => {
    if (view === 'market') {
      void loadMarketSkills()
    }
  }, [loadMarketSkills, view])

  const installedCount = useMemo(() => skills.filter((skill) => !isBuiltinSkill(skill)).length, [skills])
  const viewItems = useMemo(
    () => [
      { key: 'market', label: '市场' },
      { key: 'builtin', label: '内置' },
      { key: 'installed', label: '已安装', count: installedCount },
    ],
    [installedCount],
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

  const officeSkills = skills
    .filter((skill) => skillMatchesCenterView(skill, view))
    .filter(matchesQuery)

  const installedSkillsById = useMemo(() => new Map(skills.map((skill) => [skill.id, skill])), [skills])
  const selectedSkill = activeInstalledSkillId ? installedSkillsById.get(activeInstalledSkillId) ?? null : null
  const marketSkills = useMemo(() => {
    const uniqueItems = dedupeMarketItems(marketItems)
    if (!normalizedQuery) return uniqueItems
    return uniqueItems.filter((item) =>
      [
        item.name,
        item.pluginId,
        item.description,
        item.category,
        item.scope,
      ].some((value) => value?.toLowerCase().includes(normalizedQuery)),
    )
  }, [marketItems, normalizedQuery])
  const marketPluginIds = useMemo(() => new Set(marketItems.map((item) => item.pluginId)), [marketItems])

  const sectionTitle =
    view === 'market' ? '技能市场' : view === 'builtin' ? '内置技能' : '已安装技能'

  function isUserCreatedSkill(skill: SkillInfo) {
    return !isBuiltinSkill(skill) && skill.source === 'user' && !marketPluginIds.has(skill.id)
  }

  function isInstalledMarketSkill(skill: SkillInfo) {
    return !isBuiltinSkill(skill) && !isUserCreatedSkill(skill)
  }

  function getSkillMeta(skill: SkillInfo) {
    const normalizedCategory = skill.category || 'general'
    const label = SKILL_CATEGORIES.find((c) => c.id === normalizedCategory)?.name ?? t('skillCenter.defaultCategory')
    return label
  }

  function getSkillSourceLabel(skill: SkillInfo) {
    if (isBuiltinSkill(skill)) return '内置'
    return isUserCreatedSkill(skill) ? '自建' : '市场'
  }

  function getMarketSkillMeta(item: MarketplaceSkillItem) {
    const normalizedCategory = item.category || 'general'
    const label = SKILL_CATEGORIES.find((c) => c.id === normalizedCategory)?.name ?? t('skillCenter.defaultCategory')
    return label
  }

  const listLoading = view === 'market' ? marketLoading : isLoading && skills.length === 0
  const listError = view === 'market' ? marketError : loadError
  const listEmpty = view === 'market' ? marketSkills.length === 0 : officeSkills.length === 0

  return (
    <>
    <PageSectionShell
      topBar={
        <PageTopBar
          variant="title"
          title={(
            <div className="flex min-w-0 items-center gap-2.5">
              <span className="truncate text-[15px] font-semibold leading-[22px] text-foreground">{t('skillCenter.title')}</span>
            </div>
          )}
          trailing={(
            <>
              <div className="flex h-9 w-[240px] items-center gap-2 rounded-md border border-input bg-card px-3">
                <Search className="h-4 w-4 shrink-0 text-muted-foreground" />
                <input
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                  className="min-w-0 flex-1 bg-transparent text-sm text-foreground outline-none placeholder:text-muted-foreground"
                  placeholder={t('skillCenter.searchPlaceholder')}
                />
              </div>
              {isLoggedIn && (
                <Button
                  size="md"
                  variant="outline"
                  loading={syncing}
                  disabled={syncing}
                  data-aijia-skill-sync-trigger
                  data-testid="skills-sync-builtin"
                  onClick={() => void handleSyncAll()}
                >
                  {syncing ? t('skillCenter.syncing') : t('skillCenter.syncSkills')}
                </Button>
              )}
              <AppDropdown
                ariaLabel={t('skillCenter.importSkill')}
                trigger={
                  <Button size="md" data-aijia-skill-import-trigger>
                    {t('skillCenter.importSkill')}
                    <ChevronDown className="h-3.5 w-3.5" />
                  </Button>
                }
                items={[
                  {
                    id: 'import-dir',
                    label: t('skillCenter.importDirectory'),
                    icon: <FolderOpen className="h-4 w-4" />,
                    onSelect: () => void handleImportDirectory(),
                    dataAttrs: { 'data-aijia-skill-import-action': 'directory' },
                  },
                  {
                    id: 'import-archive',
                    label: t('skillCenter.importArchive'),
                    icon: <Package className="h-4 w-4" />,
                    onSelect: () => void handleImportArchive(),
                    dataAttrs: { 'data-aijia-skill-import-action': 'archive' },
                  },
                ]}
              />
            </>
          )}
        />
      }
    >
      <SkillOfficeSection
        title={sectionTitle}
        layout={view === 'market' ? 'grid' : 'list'}
        categoryBar={
          <SkillCategoryBar
            items={viewItems}
            activeKey={view}
            itemDataAttribute="data-aijia-skill-tab"
            onSelect={(key) => setView(key as SkillCenterView)}
          />
        }
      >
        {listLoading ? (
          <SkillCenterLoading title={t('skillCenter.loading')} />
        ) : listError ? (
          <SkillCenterState title={t('skillCenter.loadFailed')} desc={listError} actionLabel={t('skillCenter.retry')} onAction={() => view === 'market' ? void loadMarketSkills() : void loadSkills()} />
        ) : listEmpty ? (
          normalizedQuery ? (
            <SkillCenterState
              title={t('skillCenter.noMatch')}
              desc={t('skillCenter.noMatchDesc', { query: normalizedQuery })}
            />
          ) : (
            <SkillCenterState
              title={
                view === 'market'
                  ? '暂无市场技能'
                  : view === 'builtin'
                    ? '暂无内置技能'
                    : t('skillCenter.noLocalSkills')
              }
              desc={
                view === 'market'
                  ? '企业下发的技能会出现在这里；你也可以从本地目录或压缩包导入技能。'
                  : view === 'builtin'
                    ? '内置技能会随客户端版本或同步操作更新。'
                    : t('skillCenter.noLocalSkillsDesc')
              }
            />
          )
        ) : view === 'market' ? (
          marketSkills.map((item) => {
            const installedSkill = installedSkillsById.get(item.pluginId)
            const installed = Boolean(installedSkill)
            return (
              <SkillCard
                key={`${item.id}:${item.pluginId}`}
                title={item.name || item.pluginId}
                meta={getMarketSkillMeta(item)}
                desc={item.description}
                iconNode={getSkillAvatarNode(item.pluginId)}
                iconBg={getSkillCardIconBg(item.pluginId)}
                version={installedSkill?.version ?? item.version}
                skillId={item.pluginId}
                skillSource={item.scope === 'tenant' ? 'tenant' : 'global'}
                marketCard
                marketInstalled={installed}
                onClick={() => setSelectedMarketItem(item)}
                actionsSlot={
                  installed ? (
                    <Button
                      size="sm"
                      variant="ghost"
                      data-aijia-skill-market-action="added"
                      aria-label={`使用 ${installedSkill ? localizeSkill(installedSkill, i18n.language).name : item.name || item.pluginId}`}
                      onClick={() => {
                        if (installedSkill) handleUseSkill(installedSkill)
                      }}
                    >
                      <Check className="h-3.5 w-3.5 group-hover:hidden" aria-hidden />
                      <MessageSquare className="hidden h-3.5 w-3.5 group-hover:block" aria-hidden />
                      <span className="sr-only">已添加</span>
                    </Button>
                  ) : (
                    <Button
                      size="sm"
                      variant="ghost"
                      loading={installingMarketId === item.pluginId}
                      disabled={installingMarketId === item.pluginId}
                      data-aijia-skill-market-action="add"
                      aria-label={`添加 ${item.name || item.pluginId}`}
                      onClick={() => void handleInstallMarketplace(item)}
                    >
                      <Plus className="h-4 w-4" />
                    </Button>
                  )
                }
              />
            )
          })
        ) : (
          officeSkills.map((skill) => {
            const localized = localizeSkill(skill, i18n.language)
            const isUserSkill = isUserCreatedSkill(skill)
            const marketSkill = isInstalledMarketSkill(skill)
            const manageable = canToggleSkillEnablement(skill)
            const enabled = isSkillEnabled(skill)
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
                label: t('skillCenter.exportLabel'),
                icon: <Download />,
                onSelect: () => void handleExportSkill(skill.id, localized.name),
              })
              menuItems.push({
                id: 'delete',
                label: t('skillCenter.deleteSkill'),
                icon: <Trash2 />,
                className: 'text-destructive [&_svg]:text-destructive',
                onSelect: () => void handleDeleteSkill(skill.id, localized.name),
              })
            } else if (marketSkill) {
              menuItems.push({
                id: 'uninstall',
                label: '卸载技能',
                icon: <Trash2 />,
                className: 'text-destructive [&_svg]:text-destructive',
                onSelect: () => void handleDeleteSkill(skill.id, localized.name, '卸载技能'),
              })
            }
            return (
              <SkillCard
                key={skill.id}
                title={localized.name}
                meta={getSkillMeta(skill)}
                desc={localized.description}
                iconNode={getSkillAvatarNode(skill.id)}
                iconBg={getSkillCardAvatarClass(skill.id)}
                layout="row"
                version={skill.version}
                sourceLabel={getSkillSourceLabel(skill)}
                skillId={skill.id}
                skillSource={skill.source}
                skillEnabled={enabled}
                marketCard={marketSkill}
                marketInstalled
                onClick={() => setActiveInstalledSkillId(skill.id)}
                actionsSlot={
                  <div className="flex items-center gap-2">
                    {manageable ? (
                      <Switch
                        size="sm"
                        checked={enabled}
                        disabled={enablementChangingId === skill.id}
                        data-aijia-skill-toggle={skill.id}
                        aria-label={`${localized.name} 技能开关`}
                        onCheckedChange={(next) => void handleSetSkillEnabled(skill, next)}
                      />
                    ) : null}
                    {menuItems.length > 0 ? (
                      <AppDropdown
                        ariaLabel={`${localized.name} ${t('skillCenter.moreActions')}`}
                        trigger={
                          <Button
                            size="sm"
                            variant="ghost"
                            aria-label={`${localized.name} ${t('skillCenter.moreActions')}`}
                            icon={<MoreHorizontal />}
                          />
                        }
                        items={menuItems}
                      />
                    ) : null}
                  </div>
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
    <SkillDetailDialog
      open={Boolean(selectedSkill || selectedMarketItem)}
      skill={selectedSkill ?? (selectedMarketItem ? installedSkillsById.get(selectedMarketItem.pluginId) ?? null : null)}
      marketplaceItem={selectedMarketItem}
      installing={selectedMarketItem ? installingMarketId === selectedMarketItem.pluginId : false}
      onOpenChange={(open) => {
        if (!open) {
          setSelectedMarketItem(null)
          setActiveInstalledSkillId(null)
        }
      }}
      onInstall={(item) => void handleInstallMarketplace(item)}
      onUse={handleUseSkill}
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
    <div className="col-span-full rounded-md border border-dashed border-border bg-card p-6 text-sm shadow-[var(--shadow-card)]">
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

function SkillCenterLoading({ title }: { title: string }) {
  return (
    <div className="col-span-full flex min-h-36 items-center justify-center pt-6 text-sm text-muted-foreground">
      {title}
    </div>
  )
}
