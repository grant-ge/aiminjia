import { useState } from 'react'
import { CheckCheck } from 'lucide-react'

import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { Button } from '@/components/ui/button'
import { useEmployees } from '@/features/employees/useEmployees'
import { useInbox } from '@/features/employees/useInbox'
import { useUiStore } from '@/stores/uiStore'
import type { InboxKind } from '@/lib/tauri'

type KindFilter = 'all' | InboxKind

const KIND_TABS: { key: KindFilter; label: string }[] = [
  { key: 'all', label: '全部' },
  { key: 'report', label: '汇报' },
  { key: 'signal', label: '提示' },
  { key: 'running', label: '运行中' },
  { key: 'error', label: '异常' },
]

function kindIcon(kind: InboxKind): string {
  switch (kind) {
    case 'report': return '📄'
    case 'signal': return '💡'
    case 'running': return '⚙️'
    case 'error': return '⚠️'
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
  if (diffH < 24) return `今天 ${d.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })}`
  if (diffH < 48) return `昨天 ${d.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })}`
  return d.toLocaleDateString('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' })
}

export function InboxPage() {
  const { employees } = useEmployees()
  const { entries, markAllRead, markRead } = useInbox()
  const setRoute = useUiStore((s) => s.setRoute)

  const [kindFilter, setKindFilter] = useState<KindFilter>('all')
  const [empFilter, setEmpFilter] = useState<string>('all')
  const [markingAll, setMarkingAll] = useState(false)

  const filtered = entries.filter((e) => {
    if (kindFilter !== 'all' && e.kind !== kindFilter) return false
    if (empFilter !== 'all' && e.employeeId !== empFilter) return false
    return true
  })

  const unread = filtered.filter((e) => !e.read).length

  const handleMarkAll = async () => {
    setMarkingAll(true)
    try {
      const ids = [...new Set(entries.map((e) => e.employeeId))]
      await Promise.all(ids.map((id) => markAllRead(id)))
    } finally {
      setMarkingAll(false)
    }
  }

  return (
    <PageSectionShell
      topBar={
        <PageTopBar
          variant="title"
          title="汇报中心"
          trailing={
            <Button
              variant="ghost"
              size="sm"
              className="gap-1.5 text-xs"
              disabled={markingAll || unread === 0}
              onClick={handleMarkAll}
            >
              <CheckCheck className="h-3.5 w-3.5" />
              全部已读
            </Button>
          }
        />
      }
      padding="px-8 pt-5 pb-7"
      gap="gap-4"
    >
      {/* Filter bar */}
      <div className="flex items-center gap-3">
        {/* Kind tabs */}
        <div className="flex items-center gap-0.5 rounded-lg bg-muted/50 p-0.5">
          {KIND_TABS.map((tab) => (
            <button
              key={tab.key}
              type="button"
              onClick={() => setKindFilter(tab.key)}
              className={`rounded-md px-2.5 py-1 text-xs transition-colors ${
                kindFilter === tab.key
                  ? 'bg-background font-medium text-foreground shadow-sm'
                  : 'text-muted-foreground hover:text-foreground'
              }`}
            >
              {tab.label}
            </button>
          ))}
        </div>

        {/* Employee filter */}
        <select
          value={empFilter}
          onChange={(e) => setEmpFilter(e.target.value)}
          className="rounded-lg border border-border bg-background px-2.5 py-1 text-xs text-foreground"
        >
          <option value="all">所有员工</option>
          {employees.map((emp) => (
            <option key={emp.id} value={emp.id}>
              {emp.avatar} {emp.name}
            </option>
          ))}
        </select>

        {unread > 0 && (
          <span className="rounded-full bg-primary/10 px-2 py-0.5 text-xs font-medium text-primary">
            {unread} 条未读
          </span>
        )}
      </div>

      {/* Entry list */}
      {filtered.length === 0 ? (
        <div className="flex h-[240px] items-center justify-center rounded-2xl border border-dashed border-border">
          <p className="text-sm text-muted-foreground">暂无记录</p>
        </div>
      ) : (
        <div className="flex flex-col divide-y divide-border overflow-hidden rounded-2xl border border-border">
          {filtered.map((entry) => {
            const emp = employees.find((e) => e.id === entry.employeeId)
            const clickable = !!entry.conversationId
            const handleClick = async () => {
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
                className={`flex w-full items-start gap-3 px-5 py-4 text-left transition-colors hover:bg-accent/30 disabled:cursor-default disabled:hover:bg-transparent ${
                  !entry.read ? 'bg-blue-50/20' : ''
                }`}
              >
                <span className="mt-0.5 text-lg">{kindIcon(entry.kind)}</span>
                <div className="min-w-0 flex-1">
                  <div className="flex items-baseline gap-2">
                    {emp && (
                      <span className="shrink-0 text-xs font-medium text-muted-foreground">
                        {emp.avatar} {emp.name}
                      </span>
                    )}
                    <span className="text-sm font-medium text-foreground">{entry.title}</span>
                  </div>
                  {entry.summary && (
                    <p className="mt-0.5 text-xs text-muted-foreground line-clamp-2">{entry.summary}</p>
                  )}
                  {entry.catchupInfo && (
                    <p className="mt-0.5 text-xs italic text-muted-foreground/60">{entry.catchupInfo}</p>
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
    </PageSectionShell>
  )
}
