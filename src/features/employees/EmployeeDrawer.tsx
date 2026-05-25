import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { X, MessageSquare, Square, Clock, RefreshCw } from 'lucide-react'
import {
  employeeDelete,
  employeeStopRun,
  employeeTemplateCheckUpgrade,
  employeeTemplateUpgrade,
  employeeTrigger,
  employeeUpdate,
  type ChatAttachmentPayload,
  type EmployeeActiveRunInfo,
  type EmployeeRecord,
  type InboxEntry,
  type TemplateUpgradeCheck,
} from '@/lib/tauri'
import { Button } from '@/components/ui/button'
import { Sheet, SheetContent } from '@/components/ui/sheet'
import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import { deriveStatus, type EmployeeStatus } from './EmployeeCard'
import { useChatStore } from '@/stores/chatStore'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'
import { findTemplate } from './templates'
import { ResourceConfigForm } from './ResourceConfigForm'
import { runTriggerPrechecks } from './triggerPrechecks'
import { CronEditDialog } from './CronEditDialog'
import { formatRelativeNextRun } from './timeFormat'
import { seedDispatchConversation } from './seedDispatchConversation'

function statusBadgeClass(status: EmployeeStatus): string {
  switch (status) {
    case 'running': return 'bg-blue-100 text-blue-700 animate-pulse'
    case 'has-report': return 'bg-green-100 text-green-700'
    case 'needs-setup': return 'bg-orange-100 text-orange-700'
    case 'idle':
    default:
      return 'bg-muted text-muted-foreground'
  }
}

const STATUS_TEXT_KEY: Record<EmployeeStatus, string> = {
  running: 'employeeDrawer.statusRunning',
  'has-report': 'employeeDrawer.statusHasReport',
  'needs-setup': 'employeeDrawer.statusNeedsSetup',
  idle: 'employeeDrawer.statusIdle',
}

function useCronToHuman() {
  const { t } = useTranslation()
  return (cron: string): string => {
    const parts = cron.trim().split(/\s+/)
    if (parts.length !== 5) return cron
    const [minute, hour, , , dow] = parts
    const m = parseInt(minute, 10)
    const h = parseInt(hour, 10)
    if (!isNaN(m) && !isNaN(h)) {
      const timeStr = `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}`
      if (dow === '*') return t('employeeDrawer.cronEveryDay', { time: timeStr })
      if (dow === '1-5') return t('employeeDrawer.cronWeekdays', { time: timeStr })
      const dayNames = t('employeeDrawer.dayNames', { returnObjects: true }) as string[]
      const dayNums = dow.split(',').map(Number).filter((n) => !isNaN(n))
      if (dayNums.length > 0) {
        const days = dayNums.map((n) => dayNames[n] ?? '?').join('/')
        return t('employeeDrawer.cronWeekly', { days, time: timeStr })
      }
    }
    return cron
  }
}

interface EmployeeDrawerProps {
  employee: EmployeeRecord | null
  inboxEntries: InboxEntry[]
  activeRun?: EmployeeActiveRunInfo | null
  onClose: () => void
  onRefresh: () => Promise<void>
}

function formatElapsed(iso: string): string {
  const ms = Date.now() - new Date(iso).getTime()
  const s = Math.max(0, Math.floor(ms / 1000))
  if (s < 60) return `${s}s`
  const m = Math.floor(s / 60)
  const r = s % 60
  return `${m}m ${r}s`
}

export function EmployeeDrawer({ employee: emp, inboxEntries, activeRun = null, onClose, onRefresh }: EmployeeDrawerProps) {
  const { t, i18n } = useTranslation()
  const cronToHuman = useCronToHuman()
  const [busy, setBusy] = useState(false)
  const [resourceModalOpen, setResourceModalOpen] = useState(false)
  const [cronModalOpen, setCronModalOpen] = useState(false)
  const [upgradeCheck, setUpgradeCheck] = useState<TemplateUpgradeCheck | null>(null)
  const setRoute = useUiStore((s) => s.setRoute)
  const setSidebarTab = useUiStore((s) => s.setSidebarTab)
  const getSkillById = useSkillStore((s) => s.getById)

  // PR-12: probe whether a newer template version is available for this
  // employee. Runs once per opened drawer instance; cleared between
  // employees so we don't render a stale "升级" badge from the previous
  // selection.
  const empIdForProbe = emp?.id ?? null
  useEffect(() => {
    if (!empIdForProbe) {
      setUpgradeCheck(null)
      return
    }
    let cancelled = false
    setUpgradeCheck(null)
    employeeTemplateCheckUpgrade(empIdForProbe)
      .then((c) => {
        if (!cancelled) setUpgradeCheck(c)
      })
      .catch((err) => {
        console.warn('[EmployeeDrawer] check upgrade failed:', err)
      })
    return () => {
      cancelled = true
    }
  }, [empIdForProbe])

  if (!emp) return null

  const template = findTemplate(emp.templateId)
  const status = deriveStatus(emp, inboxEntries, activeRun)

  const triggerNow = async (attachments: ChatAttachmentPayload[]) => {
    const convId = await employeeTrigger(emp.id, undefined, attachments)
    // Synchronously seed chatStore so the sidebar + MessageList have a stable
    // anchor before the backend's async `conversation:created` event arrives.
    // Without this, `setRoute({chat, convId})` below can race ahead of the
    // App.tsx reload listener — the new chat page mounts, calls getMessages,
    // gets [] (the spawned agent hasn't yet persisted the dispatch prompt),
    // and the user sees a blank chat with no way to find this conversation
    // in the sidebar until the reload eventually lands.
    const chatStore = useChatStore.getState()
    const nextConversations = seedDispatchConversation(
      chatStore.conversations,
      convId,
      emp.name,
    )
    if (nextConversations !== chatStore.conversations) {
      chatStore.setConversations(nextConversations)
    }
    chatStore.setMessages([])
    await onRefresh()
    onClose()
    // Switch sidebar to 数字员工 tab so the user lands in the right section
    // when they navigate back to the home view.
    setSidebarTab('employee')
    setRoute({ kind: 'chat', conversationId: convId })
  }

  const handleTrigger = async () => {
    if (!template) {
      // Custom (non-builtin) employees — fall back to F1 behaviour: dispatch immediately.
      setBusy(true)
      try {
        await triggerNow([])
      } catch (err) {
        console.error('[EmployeeDrawer] trigger error:', err)
      } finally {
        setBusy(false)
      }
      return
    }

    setBusy(true)
    try {
      const result = runTriggerPrechecks({
        template,
        employee: emp,
      })

      switch (result.kind) {
        case 'ready':
          await triggerNow([])
          return
        case 'resource':
          setResourceModalOpen(true)
          return
        case 'knowledge-indexing':
          alert(t('employeeDrawer.knowledgeIndexing'))
          return
      }
    } catch (err) {
      console.error('[EmployeeDrawer] trigger error:', err)
    } finally {
      setBusy(false)
    }
  }

  const handleResourceSubmit = async (next: Record<string, unknown>) => {
    setResourceModalOpen(false)
    try {
      await employeeUpdate(emp.id, { resourceConfig: next })
      await onRefresh()
    } catch (err) {
      console.error('[EmployeeDrawer] resource submit error:', err)
      alert(t('employeeDrawer.saveConfigFailed', { error: String(err) }))
    }
    // Do NOT auto re-trigger — user clicks 现在派活 again to dispatch.
    // This prevents an infinite save→retrigger loop if a future
    // resourceConfigKind has a Form that submits but isResourceConfigured
    // still returns false.
  }

  const handleToggleCron = async () => {
    setBusy(true)
    try {
      await employeeUpdate(emp.id, { cronEnabled: !emp.cronEnabled })
      await onRefresh()
    } catch (err) {
      console.error('[EmployeeDrawer] toggle error:', err)
    } finally {
      setBusy(false)
    }
  }

  const handleStop = async () => {
    setBusy(true)
    try {
      await employeeStopRun(emp.id)
      await onRefresh()
    } catch (err) {
      console.error('[EmployeeDrawer] stop error:', err)
      alert(t('employeeDrawer.stopFailed', { error: String(err) }))
    } finally {
      setBusy(false)
    }
  }

  const handleCronSubmit = async (cron: string | null) => {
    setCronModalOpen(false)
    setBusy(true)
    try {
      await employeeUpdate(emp.id, { cron, cronEnabled: cron !== null })
      await onRefresh()
    } catch (err) {
      console.error('[EmployeeDrawer] cron edit error:', err)
      alert(t('employeeDrawer.cronEditFailed', { error: String(err) }))
    } finally {
      setBusy(false)
    }
  }

  const handleUpgradeTemplate = async () => {
    if (!upgradeCheck?.hasUpgrade) return
    const fields = upgradeCheck.changedFields.join('、') || t('employeeDrawer.upgradeFieldsDefault')
    const ok = await requestConfirm({
      title: t('employeeDrawer.upgrade'),
      description: t('employeeDrawer.upgradeConfirm', {
        from: upgradeCheck.currentVersion ?? '?',
        to: upgradeCheck.latestVersion,
        fields,
      }),
    })
    if (!ok) return
    setBusy(true)
    try {
      await employeeTemplateUpgrade(emp.id)
      const next = await employeeTemplateCheckUpgrade(emp.id).catch(() => null)
      setUpgradeCheck(next)
      await onRefresh()
    } catch (err) {
      console.error('[EmployeeDrawer] upgrade error:', err)
      alert(t('employeeDrawer.upgradeFailed', { error: String(err) }))
    } finally {
      setBusy(false)
    }
  }

  const handleDismiss = async () => {
    const ok = await requestConfirm({
      title: t('employeeDrawer.deleteEmployee'),
      description: t('employeeDrawer.deleteConfirm', { name: emp.name }),
      variant: 'destructive',
    })
    if (!ok) return
    setBusy(true)
    try {
      await employeeDelete(emp.id)
      await onRefresh()
      onClose()
    } catch (err) {
      console.error('[EmployeeDrawer] dismiss error:', err)
      alert(t('employeeDrawer.deleteFailed', { error: String(err) }))
    } finally {
      setBusy(false)
    }
  }

  const empInbox = inboxEntries.filter((e) => e.employeeId === emp.id).slice(0, 5)

  return (
    <Sheet open={!!emp} onOpenChange={(open) => { if (!open) onClose() }}>
      <SheetContent side="right" data-aijia-employee-drawer className="w-[520px] p-0 flex flex-col">
        {/* Header */}
        <div className="flex items-start justify-between border-b border-border p-5">
          <div className="flex items-center gap-3">
            <span className="text-3xl">{emp.avatar}</span>
            <div>
              <h2 className="text-base font-semibold text-foreground">{emp.name}</h2>
              <p className="text-sm text-muted-foreground">{emp.role}</p>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <span className={`rounded-full px-2.5 py-0.5 text-xs font-medium ${statusBadgeClass(status)}`}>
              {t(STATUS_TEXT_KEY[status])}
            </span>
            <button type="button" onClick={onClose} data-aijia-employee-action="close" className="rounded-md p-1 hover:bg-accent">
              <X className="h-4 w-4 text-muted-foreground" />
            </button>
          </div>
        </div>

        {/* Scrollable body */}
        <div className="flex-1 overflow-y-auto">
          <div className="flex flex-col gap-5 p-5">
            {/* 模板升级提示 — only shown when a newer cached / bootstrap
                template version exists for this employee. PR-12. */}
            {upgradeCheck?.hasUpgrade ? (
              <section
                data-testid="template-upgrade-banner"
                className="flex items-start gap-3 rounded-lg border border-blue-200 bg-blue-50/40 px-3 py-2.5 text-sm dark:border-blue-900 dark:bg-blue-950/40"
              >
                <RefreshCw className="mt-0.5 h-4 w-4 shrink-0 text-blue-600" aria-hidden />
                <div className="min-w-0 flex-1">
                  <p className="text-xs font-medium text-blue-900 dark:text-blue-100">
                    {t('employeeDrawer.templateUpgradeBanner', { from: upgradeCheck.currentVersion ?? '?', to: upgradeCheck.latestVersion })}
                  </p>
                  {upgradeCheck.changedFields.length > 0 ? (
                    <p className="mt-0.5 text-xs text-blue-800/80 dark:text-blue-100/70">
                      {t('employeeDrawer.templateUpgradeFields', { fields: upgradeCheck.changedFields.join('、') })}
                    </p>
                  ) : null}
                </div>
                <Button
                  size="sm"
                  variant="outline"
                  className="shrink-0"
                  disabled={busy}
                  onClick={handleUpgradeTemplate}
                >
                  {t('employeeDrawer.upgrade')}
                </Button>
              </section>
            ) : null}

            {/* 职责 */}
            <section>
              <h3 className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t('employeeDrawer.responsibility')}</h3>
              <p className="text-sm leading-relaxed text-foreground">{emp.description}</p>
            </section>

            {/* 触发器 */}
            <section>
              <h3 className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t('employeeDrawer.triggerMethod')}</h3>
              <div className="flex flex-col gap-1.5 text-sm">
                <div className="flex items-center gap-2">
                  <span className="text-muted-foreground">{t('employeeDrawer.onDemand')}</span>
                  <span className="text-foreground">{t('employeeDrawer.alwaysSupported')}</span>
                </div>
                {emp.cron ? (
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <span className="text-muted-foreground">{t('employeeDrawer.cronTrigger')}</span>
                      <span className="font-mono text-xs text-foreground">{cronToHuman(emp.cron)}</span>
                    </div>
                    <button
                      type="button"
                      data-aijia-employee-action="toggle-cron-badge"
                      onClick={handleToggleCron}
                      disabled={busy}
                      className={`rounded-full px-2.5 py-0.5 text-xs font-medium transition-colors ${
                        emp.cronEnabled
                          ? 'bg-green-100 text-green-700 hover:bg-green-200'
                          : 'bg-muted text-muted-foreground hover:bg-accent'
                      }`}
                    >
                      {emp.cronEnabled ? t('employeeDrawer.cronEnabled') : t('employeeDrawer.cronPaused')}
                    </button>
                  </div>
                ) : (
                  <div className="flex items-center gap-2">
                    <span className="text-muted-foreground">{t('employeeDrawer.cronTrigger')}</span>
                    <span className="text-muted-foreground/60">{t('employeeDrawer.cronNotConfigured')}</span>
                  </div>
                )}
              </div>
            </section>

            {/* 绑定技能 — what the employee actually does. The old UI showed
                `toolWhitelist` (internal tool names like `bash`, `load_file`)
                which leaked implementation detail and conflated capability
                (skill) with permission (tools). User-visible answer to
                "员工会做什么" is the SKILL.md it's bound to. */}
            {emp.defaultSkillId && (() => {
              const skill = getSkillById(emp.defaultSkillId)
              return (
                <section>
                  <h3 className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                    {t('employeeDrawer.boundSkill')}
                  </h3>
                  <div className="rounded-md border border-border bg-card px-3 py-2">
                    <div className="text-sm font-medium text-foreground">
                      {skill?.displayName || emp.defaultSkillId}
                    </div>
                    {skill?.shortDescription || skill?.description ? (
                      <div className="mt-0.5 text-xs text-muted-foreground">
                        {skill.shortDescription || skill.description}
                      </div>
                    ) : null}
                  </div>
                </section>
              )
            })()}

            {/* 近期汇报 */}
            {empInbox.length > 0 && (
              <section>
                <h3 className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t('employeeDrawer.recentActivity')}</h3>
                <div className="flex flex-col gap-1.5">
                  {empInbox.map((entry) => (
                    <div
                      key={entry.id}
                      className="flex items-start gap-2 rounded-lg border border-border/50 bg-card/60 px-3 py-2"
                    >
                      <span className="mt-0.5 text-sm">
                        {entry.kind === 'report' ? '📄' : entry.kind === 'signal' ? '💡' : entry.kind === 'running' ? '⚙️' : '⚠️'}
                      </span>
                      <div className="min-w-0 flex-1">
                        <p className="text-xs font-medium text-foreground">{entry.title}</p>
                        {entry.summary && (
                          <p className="mt-0.5 truncate text-xs text-muted-foreground">{entry.summary}</p>
                        )}
                        <p className="mt-0.5 text-xs text-muted-foreground/60">
                          {new Date(entry.createdAt).toLocaleString(i18n.language, { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' })}
                        </p>
                      </div>
                      {!entry.read && (
                        <span className="mt-1.5 h-1.5 w-1.5 shrink-0 rounded-full bg-blue-500" />
                      )}
                    </div>
                  ))}
                </div>
              </section>
            )}

            {/* 本月统计 */}
            <section>
              <h3 className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t('employeeDrawer.monthlyRuns')}</h3>
              <div className="grid grid-cols-2 gap-3">
                <div className="rounded-lg bg-accent/50 px-3 py-2.5">
                  <p className="text-xl font-bold text-foreground">
                    {inboxEntries.filter((e) => e.employeeId === emp.id && e.kind === 'report').length}
                  </p>
                  <p className="text-xs text-muted-foreground">{t('employeeDrawer.reportCountLabel')}</p>
                </div>
                <div className="rounded-lg bg-accent/50 px-3 py-2.5">
                  <p className="text-xl font-bold text-foreground">
                    {emp.lastRunAt ? new Date(emp.lastRunAt).toLocaleDateString(i18n.language, { month: 'numeric', day: 'numeric' }) : '—'}
                  </p>
                  <p className="text-xs text-muted-foreground">{t('employeeDrawer.lastRun')}</p>
                </div>
              </div>
            </section>
          </div>
        </div>

        {/* Footer actions */}
        <div className="border-t border-border p-4 flex flex-col gap-3">
          {activeRun ? (
            /* Running state — replaces all idle controls */
            <div className="flex flex-col gap-2">
              <div className="rounded-lg bg-blue-50 px-3 py-2 text-xs text-blue-900 dark:bg-blue-950 dark:text-blue-100">
                {t('employeeDrawer.runningElapsed', { time: formatElapsed(activeRun.startedAt) })}
              </div>
              <div className="flex items-center gap-2">
                <Button
                  variant="outline"
                  className="flex-1 gap-1.5"
                  data-aijia-employee-action="view-chat"
                  onClick={() => {
                    onClose()
                    setRoute({ kind: 'chat', conversationId: activeRun.conversationId })
                  }}
                >
                  <MessageSquare className="h-4 w-4" /> {t('employeeDrawer.jumpToChat')}
                </Button>
                <Button
                  variant="outline"
                  className="flex-1 gap-1.5 text-destructive hover:bg-destructive/10"
                  data-aijia-employee-action="stop"
                  disabled={busy}
                  onClick={handleStop}
                >
                  <Square className="h-4 w-4 fill-current" /> {t('employeeDrawer.stop')}
                </Button>
              </div>
            </div>
          ) : (
            /* Idle / scheduled / paused — primary + secondary hierarchy */
            <div className="flex flex-col gap-3">
              <Button
                className="w-full gap-1.5"
                size="lg"
                disabled={busy || emp.lifecycle === 'archived'}
                onClick={handleTrigger}
                data-aijia-employee-action="dispatch"
              >
                <MessageSquare className="h-4 w-4" />
                {emp.lifecycle === 'archived' ? t('employeeDrawer.employeeDeleted') : t('employeeDrawer.dispatchNow')}
              </Button>

              {/* Cron management row */}
              {emp.cron ? (
                <div className="flex items-center justify-between rounded-md bg-accent/40 px-3 py-2 text-xs">
                  <div className="flex min-w-0 flex-1 items-center gap-1.5 text-muted-foreground">
                    <Clock className="h-3 w-3 shrink-0" />
                    <span className="truncate">{t('employeeDrawer.cronSchedule', { schedule: cronToHuman(emp.cron) })}</span>
                    {emp.nextRunAt && emp.cronEnabled && emp.lifecycle === 'active' && (
                      <span className="shrink-0 text-muted-foreground/60">
                        · {formatRelativeNextRun(emp.nextRunAt)}
                      </span>
                    )}
                  </div>
                  <div className="flex shrink-0 items-center gap-1">
                    <button
                      type="button"
                      data-aijia-employee-action="edit-cron"
                      onClick={() => setCronModalOpen(true)}
                      className="text-muted-foreground hover:text-foreground"
                    >
                      {t('employeeDrawer.editCron')}
                    </button>
                    <span className="text-muted-foreground/40">·</span>
                    <button
                      type="button"
                      data-aijia-employee-action="toggle-cron"
                      onClick={handleToggleCron}
                      className={
                        emp.cronEnabled
                          ? 'text-muted-foreground hover:text-foreground'
                          : 'text-amber-600 hover:text-amber-700'
                      }
                      disabled={busy}
                    >
                      {emp.cronEnabled ? t('employeeDrawer.toggleCronOff') : t('employeeDrawer.toggleCronOn')}
                    </button>
                  </div>
                </div>
              ) : (
                <button
                  type="button"
                  data-aijia-employee-action="add-cron-trigger"
                  onClick={() => setCronModalOpen(true)}
                  className="flex items-center gap-1.5 self-start text-xs text-muted-foreground hover:text-foreground"
                >
                  <Clock className="h-3 w-3" /> {t('employeeDrawer.addCronTrigger')}
                </button>
              )}

              {/* Tertiary actions */}
              <div className="flex items-center justify-end gap-3 border-t border-border/50 pt-2 text-xs">
                {template && template.resourceConfigKind !== 'none' && (
                  <button
                    type="button"
                    data-aijia-employee-action="config-resource"
                    onClick={() => setResourceModalOpen(true)}
                    className="text-muted-foreground hover:text-foreground"
                  >
                    {t('employeeDrawer.configResource')}
                  </button>
                )}
                <button
                  type="button"
                  data-aijia-employee-action="fire"
                  onClick={handleDismiss}
                  disabled={busy}
                  className="text-muted-foreground hover:text-destructive"
                >
                  {t('employeeDrawer.deleteEmployee')}
                </button>
              </div>
            </div>
          )}

          <CronEditDialog
            open={cronModalOpen}
            initial={emp.cron}
            onSubmit={handleCronSubmit}
            onCancel={() => setCronModalOpen(false)}
          />
        </div>

        <ResourceConfigForm
          open={resourceModalOpen}
          kind={template?.resourceConfigKind ?? 'none'}
          initial={(emp.resourceConfig as Record<string, unknown>) ?? {}}
          onSubmit={handleResourceSubmit}
          onCancel={() => setResourceModalOpen(false)}
        />
      </SheetContent>
    </Sheet>
  )
}
