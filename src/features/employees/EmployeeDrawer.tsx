import { useState } from 'react'
import { X, MessageSquare, Square, Clock } from 'lucide-react'
import { open as openFileDialog } from '@tauri-apps/plugin-dialog'
import {
  dingtalkStatus,
  employeeDelete,
  employeeStopRun,
  employeeTrigger,
  employeeUpdate,
  type ChatAttachmentPayload,
  type EmployeeActiveRunInfo,
  type EmployeeRecord,
  type InboxEntry,
} from '@/lib/tauri'
import { Button } from '@/components/ui/button'
import { Sheet, SheetContent } from '@/components/ui/sheet'
import { deriveStatus, type EmployeeStatus } from './EmployeeCard'
import { useUiStore } from '@/stores/uiStore'
import { findTemplate } from './templates'
import { ResourceConfigForm } from './ResourceConfigForm'
import { runTriggerPrechecks } from './triggerPrechecks'
import { CronEditDialog } from './CronEditDialog'
import { formatRelativeNextRun } from './timeFormat'

function detectFileType(path: string): ChatAttachmentPayload['fileType'] {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  if (['xlsx', 'xls'].includes(ext)) return 'excel'
  if (ext === 'csv') return 'csv'
  if (['docx', 'doc'].includes(ext)) return 'word'
  if (ext === 'pdf') return 'pdf'
  if (ext === 'json') return 'json'
  // Should never reach here — picker filters constrain extensions.
  // Log for diagnostics; default to 'pdf' as a reasonable best effort
  // for arbitrary bytes (the LLM will see the file content via load_file
  // regardless of fileType hint).
  console.warn('[EmployeeDrawer] detectFileType: unknown extension', { path, ext })
  return 'pdf'
}

function statusBadgeClass(status: EmployeeStatus): string {
  switch (status) {
    case 'running': return 'bg-blue-100 text-blue-700 animate-pulse'
    case 'has-report': return 'bg-green-100 text-green-700'
    case 'paused': return 'bg-slate-200 text-slate-600'
    case 'needs-setup': return 'bg-orange-100 text-orange-700'
    case 'scheduled': return 'bg-amber-100 text-amber-800'
    case 'archived': return 'bg-slate-200 text-slate-400'
    case 'idle':
    default:
      return 'bg-muted text-muted-foreground'
  }
}

function statusText(status: EmployeeStatus): string {
  switch (status) {
    case 'running': return '🔵 运行中'
    case 'has-report': return '🟢 有汇报'
    case 'paused': return '⏸ 已暂停'
    case 'needs-setup': return '🟠 需要配置'
    case 'scheduled': return '🟡 定时待发'
    case 'archived': return '🗑 已解雇'
    case 'idle':
    default:
      return '⚪ 空闲'
  }
}

function cronToHuman(cron: string): string {
  const parts = cron.trim().split(/\s+/)
  if (parts.length !== 5) return cron
  const [minute, hour, , , dow] = parts
  const m = parseInt(minute, 10)
  const h = parseInt(hour, 10)
  if (!isNaN(m) && !isNaN(h)) {
    const timeStr = `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}`
    if (dow === '*') return `每天 ${timeStr}`
    if (dow === '1-5') return `工作日 ${timeStr}`
    const dayNames = ['日', '一', '二', '三', '四', '五', '六']
    const dayNums = dow.split(',').map(Number).filter((n) => !isNaN(n))
    if (dayNums.length > 0) {
      return `每周${dayNums.map((n) => dayNames[n] ?? '?').join('/')} ${timeStr}`
    }
  }
  return cron
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
  const [busy, setBusy] = useState(false)
  const [resourceModalOpen, setResourceModalOpen] = useState(false)
  const [cronModalOpen, setCronModalOpen] = useState(false)
  const setRoute = useUiStore((s) => s.setRoute)

  if (!emp) return null

  const template = findTemplate(emp.templateId)
  const status = deriveStatus(emp, inboxEntries, activeRun)

  const triggerNow = async (attachments: ChatAttachmentPayload[]) => {
    const convId = await employeeTrigger(emp.id, undefined, attachments)
    await onRefresh()
    onClose()
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
      const dt = template.requiresDingtalk ? await dingtalkStatus() : { connected: false }
      const result = runTriggerPrechecks({
        template,
        employee: emp,
        dingtalkConnected: !!dt.connected,
      })

      switch (result.kind) {
        case 'ready':
          await triggerNow([])
          return
        case 'attachments': {
          const exts = result.spec.accept.split(',').map((s) => s.trim().replace(/^\./, ''))
          const picked = await openFileDialog({
            multiple: result.spec.max > 1,
            filters: [{ name: '允许的文件', extensions: exts }],
          })
          if (!picked) return  // user cancelled
          const paths = Array.isArray(picked) ? picked : [picked]
          if (paths.length < result.spec.min || paths.length > result.spec.max) {
            alert(`请选择 ${result.spec.min}-${result.spec.max} 个文件`)
            return
          }
          const attachments: ChatAttachmentPayload[] = paths.map((p, i) => ({
            id: `picker-${Date.now()}-${i}`,
            fileName: p.split(/[\\/]/).pop() ?? p,
            filePath: p,
            kind: 'file',
            fileSize: 0,  // unknown without an extra stat call; backend re-reads from disk
            fileType: detectFileType(p),
          }))
          await triggerNow(attachments)
          return
        }
        case 'resource':
          setResourceModalOpen(true)
          return
        case 'dingtalk':
          alert('该员工需要钉钉账号授权，请前往 设置 → 钉钉账号 完成授权后再试。')
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
      alert(`保存配置失败：${String(err)}`)
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
      alert(`停止失败：${String(err)}`)
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
      alert(`修改失败：${String(err)}`)
    } finally {
      setBusy(false)
    }
  }

  const handlePauseEmployee = async () => {
    const next: 'active' | 'paused' = emp.lifecycle === 'paused' ? 'active' : 'paused'
    setBusy(true)
    try {
      await employeeUpdate(emp.id, { lifecycle: next })
      await onRefresh()
    } catch (err) {
      console.error('[EmployeeDrawer] pause error:', err)
      alert(`操作失败：${String(err)}`)
    } finally {
      setBusy(false)
    }
  }

  const handleDismiss = async () => {
    if (!confirm(`确定解雇「${emp.name}」吗？\n\n该员工将在 7 天后从系统中清除，期间可在首页"已解雇"区一键恢复。`)) {
      return
    }
    setBusy(true)
    try {
      await employeeDelete(emp.id)
      await onRefresh()
      onClose()
    } catch (err) {
      console.error('[EmployeeDrawer] dismiss error:', err)
      alert(`解雇失败：${String(err)}`)
    } finally {
      setBusy(false)
    }
  }

  const empInbox = inboxEntries.filter((e) => e.employeeId === emp.id).slice(0, 5)

  return (
    <Sheet open={!!emp} onOpenChange={(open) => { if (!open) onClose() }}>
      <SheetContent side="right" className="w-[520px] p-0 flex flex-col">
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
              {statusText(status)}
            </span>
            <button type="button" onClick={onClose} className="rounded-md p-1 hover:bg-accent">
              <X className="h-4 w-4 text-muted-foreground" />
            </button>
          </div>
        </div>

        {/* Scrollable body */}
        <div className="flex-1 overflow-y-auto">
          <div className="flex flex-col gap-5 p-5">
            {/* 职责 */}
            <section>
              <h3 className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">职责</h3>
              <p className="text-sm leading-relaxed text-foreground">{emp.description}</p>
            </section>

            {/* 触发器 */}
            <section>
              <h3 className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">触发方式</h3>
              <div className="flex flex-col gap-1.5 text-sm">
                <div className="flex items-center gap-2">
                  <span className="text-muted-foreground">按需派活</span>
                  <span className="text-foreground">✓ 始终支持</span>
                </div>
                {emp.cron ? (
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <span className="text-muted-foreground">定时触发</span>
                      <span className="font-mono text-xs text-foreground">{cronToHuman(emp.cron)}</span>
                    </div>
                    <button
                      type="button"
                      onClick={handleToggleCron}
                      disabled={busy}
                      className={`rounded-full px-2.5 py-0.5 text-xs font-medium transition-colors ${
                        emp.cronEnabled
                          ? 'bg-green-100 text-green-700 hover:bg-green-200'
                          : 'bg-muted text-muted-foreground hover:bg-accent'
                      }`}
                    >
                      {emp.cronEnabled ? '已启用' : '已暂停'}
                    </button>
                  </div>
                ) : (
                  <div className="flex items-center gap-2">
                    <span className="text-muted-foreground">定时触发</span>
                    <span className="text-muted-foreground/60">未配置</span>
                  </div>
                )}
              </div>
            </section>

            {/* 工具白名单 */}
            {emp.toolWhitelist.length > 0 && (
              <section>
                <h3 className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">工具白名单</h3>
                <div className="flex flex-wrap gap-1.5">
                  {emp.toolWhitelist.map((t) => (
                    <span key={t} className="rounded-md bg-accent px-2 py-0.5 font-mono text-xs text-foreground">
                      {t}
                    </span>
                  ))}
                </div>
              </section>
            )}

            {/* 近期汇报 */}
            {empInbox.length > 0 && (
              <section>
                <h3 className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">近期动态</h3>
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
                          {new Date(entry.createdAt).toLocaleString('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' })}
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
              <h3 className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">本月运行</h3>
              <div className="grid grid-cols-2 gap-3">
                <div className="rounded-lg bg-accent/50 px-3 py-2.5">
                  <p className="text-xl font-bold text-foreground">
                    {inboxEntries.filter((e) => e.employeeId === emp.id && e.kind === 'report').length}
                  </p>
                  <p className="text-xs text-muted-foreground">次汇报</p>
                </div>
                <div className="rounded-lg bg-accent/50 px-3 py-2.5">
                  <p className="text-xl font-bold text-foreground">
                    {emp.lastRunAt ? new Date(emp.lastRunAt).toLocaleDateString('zh-CN', { month: 'numeric', day: 'numeric' }) : '—'}
                  </p>
                  <p className="text-xs text-muted-foreground">上次运行</p>
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
                ⚡ 运行中 · 已跑 {formatElapsed(activeRun.startedAt)}
              </div>
              <div className="flex items-center gap-2">
                <Button
                  variant="outline"
                  className="flex-1 gap-1.5"
                  onClick={() => {
                    onClose()
                    setRoute({ kind: 'chat', conversationId: activeRun.conversationId })
                  }}
                >
                  <MessageSquare className="h-4 w-4" /> 跳到对话
                </Button>
                <Button
                  variant="outline"
                  className="flex-1 gap-1.5 text-destructive hover:bg-destructive/10"
                  disabled={busy}
                  onClick={handleStop}
                >
                  <Square className="h-4 w-4 fill-current" /> 停止
                </Button>
              </div>
            </div>
          ) : (
            /* Idle / scheduled / paused — primary + secondary hierarchy */
            <div className="flex flex-col gap-3">
              <Button
                className="w-full gap-1.5"
                size="lg"
                disabled={busy || emp.lifecycle === 'paused' || emp.lifecycle === 'archived'}
                onClick={handleTrigger}
              >
                <MessageSquare className="h-4 w-4" />
                {emp.lifecycle === 'paused'
                  ? '员工已暂停 — 先恢复才能派活'
                  : emp.lifecycle === 'archived'
                    ? '员工已解雇'
                    : '现在派活'}
              </Button>

              {/* Cron management row */}
              {emp.cron ? (
                <div className="flex items-center justify-between rounded-md bg-accent/40 px-3 py-2 text-xs">
                  <div className="flex min-w-0 flex-1 items-center gap-1.5 text-muted-foreground">
                    <Clock className="h-3 w-3 shrink-0" />
                    <span className="truncate">定时 {cronToHuman(emp.cron)}</span>
                    {emp.nextRunAt && emp.cronEnabled && emp.lifecycle === 'active' && (
                      <span className="shrink-0 text-muted-foreground/60">
                        · {formatRelativeNextRun(emp.nextRunAt)}
                      </span>
                    )}
                  </div>
                  <div className="flex shrink-0 items-center gap-1">
                    <button
                      type="button"
                      onClick={() => setCronModalOpen(true)}
                      className="text-muted-foreground hover:text-foreground"
                    >
                      修改
                    </button>
                    <span className="text-muted-foreground/40">·</span>
                    <button
                      type="button"
                      onClick={handleToggleCron}
                      className={
                        emp.cronEnabled
                          ? 'text-muted-foreground hover:text-foreground'
                          : 'text-amber-600 hover:text-amber-700'
                      }
                      disabled={busy}
                    >
                      {emp.cronEnabled ? '关闭' : '开启'}
                    </button>
                  </div>
                </div>
              ) : (
                <button
                  type="button"
                  onClick={() => setCronModalOpen(true)}
                  className="flex items-center gap-1.5 self-start text-xs text-muted-foreground hover:text-foreground"
                >
                  <Clock className="h-3 w-3" /> 添加定时触发
                </button>
              )}

              {/* Tertiary actions */}
              <div className="flex items-center justify-between border-t border-border/50 pt-2 text-xs">
                <button
                  type="button"
                  onClick={handlePauseEmployee}
                  disabled={busy}
                  className="text-muted-foreground hover:text-foreground"
                >
                  {emp.lifecycle === 'paused' ? '▶ 恢复员工' : '⏸ 暂停员工'}
                </button>
                <div className="flex items-center gap-3">
                  {template && template.resourceConfigKind !== 'none' && (
                    <button
                      type="button"
                      onClick={() => setResourceModalOpen(true)}
                      className="text-muted-foreground hover:text-foreground"
                    >
                      ⚙️ 配置资源
                    </button>
                  )}
                  <button
                    type="button"
                    onClick={handleDismiss}
                    disabled={busy}
                    className="text-muted-foreground hover:text-destructive"
                  >
                    🗑 解雇
                  </button>
                </div>
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
