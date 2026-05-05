import { useState } from 'react'
import { RotateCcw, Trash2 } from 'lucide-react'

import { employeePurge, employeeRestore, type EmployeeRecord } from '@/lib/tauri'

interface ArchivedEmployeeCardProps {
  emp: EmployeeRecord
  onChanged: () => Promise<void> | void
}

function formatArchivedAgo(updatedAt: string): string {
  const ms = Date.now() - new Date(updatedAt).getTime()
  const days = Math.floor(ms / 86_400_000)
  if (days <= 0) return '今天'
  if (days === 1) return '1 天前'
  return `${days} 天前`
}

export function ArchivedEmployeeCard({ emp, onChanged }: ArchivedEmployeeCardProps) {
  const [busy, setBusy] = useState(false)

  const handleRestore = async () => {
    setBusy(true)
    try {
      await employeeRestore(emp.id)
      await onChanged()
    } catch (err) {
      console.error('[ArchivedEmployeeCard] restore failed:', err)
      alert(`恢复失败：${String(err)}`)
    } finally {
      setBusy(false)
    }
  }

  const handlePurge = async () => {
    if (!confirm(`确定永久删除「${emp.name}」吗？此操作不可撤销。`)) return
    setBusy(true)
    try {
      await employeePurge(emp.id)
      await onChanged()
    } catch (err) {
      console.error('[ArchivedEmployeeCard] purge failed:', err)
      alert(`删除失败：${String(err)}`)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="flex flex-col gap-2 rounded-xl border border-dashed border-border/60 bg-muted/30 p-3 opacity-70 transition-opacity hover:opacity-100">
      <div className="flex items-center gap-2">
        <span className="text-xl grayscale">{emp.avatar}</span>
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-medium text-foreground/80">{emp.name}</p>
          <p className="text-xs text-muted-foreground">{emp.role}</p>
        </div>
      </div>
      <p className="text-xs text-muted-foreground/70">解雇于 {formatArchivedAgo(emp.updatedAt)}</p>
      <div className="flex items-center gap-1">
        <button
          type="button"
          onClick={handleRestore}
          disabled={busy}
          className="flex flex-1 items-center justify-center gap-1 rounded-md bg-background px-2 py-1 text-xs hover:bg-accent disabled:opacity-50"
        >
          <RotateCcw className="h-3 w-3" /> 恢复
        </button>
        <button
          type="button"
          onClick={handlePurge}
          disabled={busy}
          className="flex flex-1 items-center justify-center gap-1 rounded-md bg-background px-2 py-1 text-xs text-destructive hover:bg-destructive/10 disabled:opacity-50"
        >
          <Trash2 className="h-3 w-3" /> 永久删除
        </button>
      </div>
    </div>
  )
}
