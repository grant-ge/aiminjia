import { useState } from 'react'
import { Inbox } from 'lucide-react'

import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { Button } from '@/components/ui/button'
import { useUiStore } from '@/stores/uiStore'
import { useEmployees } from '@/features/employees/useEmployees'
import { useInbox } from '@/features/employees/useInbox'
import { EmployeeCard, AddEmployeeCard } from '@/features/employees/EmployeeCard'
import { EmployeeDrawer } from '@/features/employees/EmployeeDrawer'
import { HireWizard } from '@/features/employees/HireWizard'
import type { EmployeeRecord } from '@/lib/tauri'

// ─── daily feed ──────────────────────────────────────────────────────────────

function kindIcon(kind: string): string {
  switch (kind) {
    case 'report': return '📄'
    case 'signal': return '💡'
    case 'running': return '⚙️'
    case 'error': return '⚠️'
    default: return '•'
  }
}

function timeLabel(iso: string): string {
  const d = new Date(iso)
  const now = new Date()
  const diffMs = now.getTime() - d.getTime()
  const diffMin = Math.floor(diffMs / 60_000)
  if (diffMin < 1) return '刚刚'
  if (diffMin < 60) return `${diffMin} 分钟前`
  const diffH = Math.floor(diffMin / 60)
  if (diffH < 24) return `${diffH} 小时前`
  const diffD = Math.floor(diffH / 24)
  if (diffD === 1) return '昨天'
  return d.toLocaleDateString('zh-CN', { month: 'numeric', day: 'numeric' })
}

// ─── greeting ─────────────────────────────────────────────────────────────────

function greeting(): string {
  const h = new Date().getHours()
  if (h < 6) return '夜深了'
  if (h < 12) return '早上好'
  if (h < 14) return '中午好'
  if (h < 18) return '下午好'
  return '晚上好'
}

// ─── HomePage ─────────────────────────────────────────────────────────────────

export function HomePage() {
  const setRoute = useUiStore((s) => s.setRoute)
  const { employees, loading: empLoading, refresh: refreshEmp } = useEmployees()
  const { entries, unreadCount, refresh: refreshInbox, markRead } = useInbox()

  const [selectedEmp, setSelectedEmp] = useState<EmployeeRecord | null>(null)
  const [hireOpen, setHireOpen] = useState(false)

  const handleRefreshAll = async () => {
    await Promise.all([refreshEmp(), refreshInbox()])
  }

  const todayEntries = entries.filter((e) => {
    const d = new Date(e.createdAt)
    const now = new Date()
    return (
      d.getFullYear() === now.getFullYear() &&
      d.getMonth() === now.getMonth() &&
      d.getDate() === now.getDate()
    )
  })

  const runningCount = entries.filter((e) => e.kind === 'running').length
  const reportCount = entries.filter((e) => e.kind === 'report').length

  return (
    <PageSectionShell
      topBar={<div data-tauri-drag-region className="h-8 shrink-0" />}
      padding="px-8 pt-5 pb-7"
      gap="gap-6"
      maxWidthClass="max-w-[900px]"
    >
      {/* ── 顶部 greeting ── */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-bold text-foreground">{greeting()}，今天</h1>
          <p className="mt-0.5 text-sm text-muted-foreground">
            {runningCount > 0 ? `${runningCount} 位员工工作中 · ` : ''}
            {reportCount > 0 ? `${reportCount} 份汇报` : '所有员工空闲'}
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          className="gap-1.5"
          onClick={() => setRoute({ kind: 'inbox' })}
        >
          <Inbox className="h-3.5 w-3.5" />
          汇报中心
          {unreadCount > 0 && (
            <span className="ml-0.5 rounded-full bg-primary px-1.5 py-0.5 text-[10px] font-medium text-primary-foreground">
              {unreadCount > 99 ? '99+' : unreadCount}
            </span>
          )}
        </Button>
      </div>

      {/* ── 员工卡片栏 ── */}
      <section>
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-sm font-semibold text-foreground">我的数字员工</h2>
          <button
            type="button"
            className="text-xs text-muted-foreground hover:text-foreground"
            onClick={() => setHireOpen(true)}
          >
            模板市场
          </button>
        </div>

        {empLoading ? (
          <div className="grid grid-cols-3 gap-3">
            {[...Array(3)].map((_, i) => (
              <div key={i} className="h-[152px] animate-pulse rounded-2xl bg-muted/60" />
            ))}
          </div>
        ) : (
          <div className="grid grid-cols-3 gap-3">
            {employees.map((emp) => (
              <EmployeeCard
                key={emp.id}
                employee={emp}
                inboxEntries={entries}
                onClick={() => setSelectedEmp(emp)}
                onRefresh={handleRefreshAll}
              />
            ))}
            <AddEmployeeCard onClick={() => setHireOpen(true)} />
          </div>
        )}
      </section>

      {/* ── 今日动态 ── */}
      <section>
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-sm font-semibold text-foreground">今日动态</h2>
          {entries.length > todayEntries.length && (
            <button
              type="button"
              className="text-xs text-muted-foreground hover:text-foreground"
              onClick={() => setRoute({ kind: 'inbox' })}
            >
              查看全部
            </button>
          )}
        </div>

        {todayEntries.length === 0 ? (
          <div className="flex h-[120px] items-center justify-center rounded-2xl border border-dashed border-border">
            <p className="text-sm text-muted-foreground">今天还没有任何动态</p>
          </div>
        ) : (
          <div className="flex flex-col divide-y divide-border overflow-hidden rounded-2xl border border-border">
            {todayEntries.slice(0, 8).map((entry) => {
              const emp = employees.find((e) => e.id === entry.employeeId)
              const clickable = !!entry.conversationId
              const handleClick = () => {
                if (!entry.read) {
                  void markRead(entry.employeeId, entry.id)
                }
                if (entry.conversationId) {
                  setRoute({ kind: 'chat', conversationId: entry.conversationId })
                }
              }
              return (
                <button
                  key={entry.id}
                  type="button"
                  onClick={handleClick}
                  disabled={!clickable}
                  className="flex w-full items-start gap-3 px-4 py-3 text-left transition-colors hover:bg-accent/30 disabled:cursor-default disabled:hover:bg-transparent"
                >
                  <span className="mt-0.5 text-base">{kindIcon(entry.kind)}</span>
                  <div className="min-w-0 flex-1">
                    <div className="flex items-baseline gap-2">
                      {emp && (
                        <span className="shrink-0 text-xs font-medium text-muted-foreground">
                          {emp.avatar} {emp.name}
                        </span>
                      )}
                      <span className="truncate text-sm text-foreground">{entry.title}</span>
                    </div>
                    {entry.summary && (
                      <p className="mt-0.5 truncate text-xs text-muted-foreground">{entry.summary}</p>
                    )}
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    <span className="text-xs text-muted-foreground/60">{timeLabel(entry.createdAt)}</span>
                    {!entry.read && (
                      <span className="h-1.5 w-1.5 rounded-full bg-blue-500" />
                    )}
                  </div>
                </button>
              )
            })}
          </div>
        )}
      </section>

      {/* ── Drawer ── */}
      <EmployeeDrawer
        employee={selectedEmp}
        inboxEntries={entries}
        onClose={() => setSelectedEmp(null)}
        onRefresh={handleRefreshAll}
      />

      {/* ── HireWizard ── */}
      <HireWizard
        open={hireOpen}
        onClose={() => setHireOpen(false)}
        onHired={handleRefreshAll}
      />
    </PageSectionShell>
  )
}
