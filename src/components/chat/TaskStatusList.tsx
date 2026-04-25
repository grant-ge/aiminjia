import { useTranslation } from 'react-i18next'

import type { ConversationTaskState } from '@/stores/streamingStore'

interface TaskStatusListProps {
  tasks: ConversationTaskState[]
}

function StatusIcon({ status }: { status: string }) {
  switch (status) {
    case 'in_progress':
    case 'running':
      return (
        <svg
          role="img"
          aria-label="running"
          className="h-3 w-3 animate-spin shrink-0"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2.5"
        >
          <circle cx="12" cy="12" r="10" strokeDasharray="50" strokeDashoffset="20" strokeLinecap="round" />
        </svg>
      )
    case 'completed':
      return (
        <span
          role="img"
          aria-label="completed"
          className="shrink-0 text-xs font-bold"
          style={{ color: 'var(--color-semantic-green, #22c55e)' }}
        >
          ✓
        </span>
      )
    case 'failed':
      return (
        <span
          role="img"
          aria-label="failed"
          className="shrink-0 text-xs font-bold"
          style={{ color: 'var(--color-semantic-red, #ef4444)' }}
        >
          ✗
        </span>
      )
    case 'pending':
      return <span role="img" aria-label="pending" className="shrink-0 text-xs opacity-50">⏳</span>
    default:
      return <span role="img" aria-label="unknown" className="shrink-0 text-xs opacity-40">•</span>
  }
}

export function TaskStatusList({ tasks }: TaskStatusListProps) {
  const { t } = useTranslation()

  if (tasks.length === 0) return null

  const runningCount = tasks.filter((task) => task.status === 'running' || task.status === 'in_progress').length
  const isOpen = runningCount > 0

  const summaryText = runningCount > 0
    ? t('streaming.tasks.running', { count: runningCount, defaultValue: `${runningCount} task(s) running` })
    : t('streaming.tasks.done', { count: tasks.length, defaultValue: `${tasks.length} task(s) done` })

  return (
    <details
      open={isOpen}
      className="mt-2 text-xs"
      style={{ color: 'var(--color-text-muted)' }}
    >
      <summary className="cursor-pointer select-none list-none hover:opacity-80">
        {summaryText}
      </summary>
      <ul className="mt-1 flex flex-col gap-0.5 pl-1">
        {tasks.map((task, index) => (
          <li key={task.taskId} className="flex items-center gap-1.5">
            <StatusIcon status={task.status} />
            <span>子任务 #{index + 1}</span>
            <span className="opacity-50" title={task.taskId} aria-label={`task-id-hint-${index + 1}`}>
              ({task.taskId.slice(-8)})
            </span>
          </li>
        ))}
      </ul>
    </details>
  )
}
