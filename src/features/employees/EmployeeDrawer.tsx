import { useState } from 'react'
import { X, Play, Pause, MessageSquare } from 'lucide-react'
import { employeeTrigger, employeeUpdate, type EmployeeRecord, type InboxEntry } from '@/lib/tauri'
import { Button } from '@/components/ui/button'
import { Sheet, SheetContent } from '@/components/ui/sheet'
import { deriveStatus, type EmployeeStatus } from './EmployeeCard'
import { useUiStore } from '@/stores/uiStore'

function statusBadgeClass(status: EmployeeStatus): string {
  switch (status) {
    case 'working': return 'bg-blue-100 text-blue-700'
    case 'has-report': return 'bg-green-100 text-green-700'
    case 'scheduled': return 'bg-amber-100 text-amber-700'
    case 'needs-setup': return 'bg-orange-100 text-orange-700'
    default: return 'bg-muted text-muted-foreground'
  }
}

function statusText(status: EmployeeStatus): string {
  switch (status) {
    case 'working': return '🔵 工作中'
    case 'has-report': return '🟢 有新汇报'
    case 'scheduled': return '🟡 定时驻场'
    case 'needs-setup': return '🟠 需要配置'
    default: return '⚪ 空闲'
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
  onClose: () => void
  onRefresh: () => Promise<void>
}

export function EmployeeDrawer({ employee: emp, inboxEntries, onClose, onRefresh }: EmployeeDrawerProps) {
  const [busy, setBusy] = useState(false)
  const setRoute = useUiStore((s) => s.setRoute)

  if (!emp) return null

  const status = deriveStatus(emp, inboxEntries)

  const handleTrigger = async () => {
    setBusy(true)
    try {
      const convId = await employeeTrigger(emp.id)
      await onRefresh()
      onClose()
      setRoute({ kind: 'chat', conversationId: convId })
    } catch (err) {
      console.error('[EmployeeDrawer] trigger error:', err)
    } finally {
      setBusy(false)
    }
  }

  const handleToggle = async () => {
    setBusy(true)
    try {
      await employeeUpdate(emp.id, { enabled: !emp.enabled })
      await onRefresh()
    } catch (err) {
      console.error('[EmployeeDrawer] toggle error:', err)
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
                      onClick={handleToggle}
                      disabled={busy}
                      className={`rounded-full px-2.5 py-0.5 text-xs font-medium transition-colors ${
                        emp.enabled
                          ? 'bg-green-100 text-green-700 hover:bg-green-200'
                          : 'bg-muted text-muted-foreground hover:bg-accent'
                      }`}
                    >
                      {emp.enabled ? '已启用' : '已暂停'}
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
        <div className="flex items-center gap-2 border-t border-border p-4">
          <Button
            className="flex-1 gap-1.5"
            disabled={busy || status === 'working'}
            onClick={handleTrigger}
          >
            <MessageSquare className="h-4 w-4" />
            现在派活
          </Button>
          {emp.cron && (
            <Button variant="outline" disabled={busy} onClick={handleToggle}>
              {emp.enabled ? <Pause className="h-4 w-4" /> : <Play className="h-4 w-4" />}
            </Button>
          )}
        </div>
      </SheetContent>
    </Sheet>
  )
}
