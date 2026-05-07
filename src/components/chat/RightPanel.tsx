import { useMemo, useState } from 'react'
import {
  ChevronDown,
  CheckCircle2,
  XCircle,
  MinusCircle,
  Circle,
  Loader2,
  File,
  FileSpreadsheet,
  FileText,
  Image,
} from 'lucide-react'

import { FilePreviewPane } from './FilePreviewPane'
import {
  isPreviewActionEnabledForFile,
  isPreviewableFileType,
  toPreviewTarget,
  type PreviewTarget,
} from './generatedFileActions'
import { cn } from '@/lib/utils'
import { useChatStore } from '@/stores/chatStore'
import type { ConversationTaskState } from '@/stores/streamingStore'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'
import type { GeneratedFile } from '@/types/message'

const EMPTY_TASKS: ConversationTaskState[] = []

// ─── RightPanel root ──────────────────────────────────────────────────────────

interface RightPanelProps {
  conversationId: string
  onOpenExternal?: (target: PreviewTarget) => void
}

export function RightPanel({ conversationId, onOpenExternal }: RightPanelProps) {
  const target = useGeneratedFilePreviewStore((s) => s.target)
  const closePreview = useGeneratedFilePreviewStore((s) => s.closePreview)
  const previewOpen = target?.conversationId === conversationId

  if (previewOpen) {
    return (
      <div
        data-testid="right-panel"
        className="flex h-full w-[720px] shrink-0 overflow-hidden border-l border-border bg-background"
      >
        <div className="min-w-0 flex-1">
          <FilePreviewPane
            target={target}
            onOpenExternal={onOpenExternal}
            onClosePreview={closePreview}
          />
        </div>
        <div className="flex h-full w-[260px] shrink-0 flex-col overflow-y-auto border-l border-border bg-background">
          <div className="px-4 py-2">
            <h2 className="text-[15px] font-semibold text-foreground">任务监控</h2>
          </div>
          <TaskSection conversationId={conversationId} />
          <ArtifactSection conversationId={conversationId} onOpenExternal={onOpenExternal} />
        </div>
      </div>
    )
  }

  return (
    <div
      data-testid="right-panel"
      className="flex h-full w-[260px] shrink-0 flex-col overflow-y-auto border-l border-border bg-background"
    >
      <div className="px-4 py-2">
        <h2 className="text-[0.9375rem] font-semibold text-foreground">任务监控</h2>
      </div>
      <TaskSection conversationId={conversationId} />
      <ArtifactSection conversationId={conversationId} onOpenExternal={onOpenExternal} />
    </div>
  )
}

// ─── TaskSection ──────────────────────────────────────────────────────────────

function TaskSection({ conversationId }: { conversationId: string }) {
  const tasks = useChatStore((s) => s.taskStates[conversationId] ?? EMPTY_TASKS)
  const hasRunning = tasks.some((t) => isRunningTaskStatus(t.status))
  const [userCollapsed, setUserCollapsed] = useState(false)
  const open = hasRunning || !userCollapsed

  return (
    <div className="border-b border-border">
      <button
        type="button"
        onClick={() => !hasRunning && setUserCollapsed((v) => !v)}
        className="flex w-full items-center justify-between px-4 py-3 text-left"
      >
        <span className="text-[0.8125rem] font-semibold text-foreground">待办</span>
        <ChevronDown
          className={cn(
            'h-4 w-4 text-muted-foreground transition-transform duration-150',
            !open && '-rotate-90',
          )}
        />
      </button>
      {open && (
        <div className="px-4 pb-3">
          {tasks.length === 0 ? (
            <p className="text-xs text-muted-foreground">暂无待办</p>
          ) : (
            <div className="flex flex-col gap-0.5">
              {tasks.map((task) => (
                <TaskItem key={task.taskId} task={task} />
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function TaskItem({ task }: { task: ConversationTaskState }) {
  return (
    <div className="flex items-start gap-2 py-1.5">
      <TaskStatusIcon status={task.status} />
      <div className="flex min-w-0 flex-col gap-0.5">
        <span
          className={cn(
            'text-xs font-medium leading-tight',
            task.status === 'completed'
              ? 'text-muted-foreground line-through'
              : 'text-foreground',
          )}
        >
          {task.subject}
        </span>
        {isRunningTaskStatus(task.status) && task.activeForm && (
          <span className="text-[0.6875rem] text-primary">{task.activeForm}</span>
        )}
      </div>
    </div>
  )
}

function TaskStatusIcon({ status }: { status: string }) {
  switch (status) {
    case 'in_progress':
    case 'running':
      return <Loader2 className="mt-0.5 h-3 w-3 shrink-0 animate-spin text-primary" />
    case 'completed':
      return (
        <CheckCircle2 className="mt-0.5 h-3 w-3 shrink-0" style={{ color: '#16A34A' }} />
      )
    case 'failed':
      return <XCircle className="mt-0.5 h-3 w-3 shrink-0 text-destructive" />
    case 'cancelled':
      return <MinusCircle className="mt-0.5 h-3 w-3 shrink-0 text-muted-foreground" />
    default:
      return <Circle className="mt-0.5 h-3 w-3 shrink-0 text-muted-foreground/40" />
  }
}

function isRunningTaskStatus(status: string) {
  return status === 'running' || status === 'in_progress'
}

// ─── ArtifactSection ──────────────────────────────────────────────────────────

function ArtifactSection({
  conversationId,
  onOpenExternal,
}: {
  conversationId: string
  onOpenExternal?: (target: PreviewTarget) => void
}) {
  const messages = useChatStore((s) => s.messages)
  const [open, setOpen] = useState(true)

  const files = useMemo(() => {
    const seen = new Set<string>()
    const result: GeneratedFile[] = []
    for (const msg of messages) {
      if (msg.conversationId !== conversationId) continue
      const generated =
        msg.content && typeof msg.content === 'object'
          ? (msg.content as { generatedFiles?: GeneratedFile[] }).generatedFiles
          : undefined
      for (const f of generated ?? []) {
        if (!seen.has(f.id) && f.isLatest) {
          seen.add(f.id)
          result.push(f)
        }
      }
    }
    return result
  }, [conversationId, messages])

  return (
    <div className="border-b border-border">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center justify-between px-4 py-3 text-left"
      >
        <span className="text-[0.8125rem] font-semibold text-foreground">产物</span>
        <ChevronDown
          className={cn(
            'h-4 w-4 text-muted-foreground transition-transform duration-150',
            !open && '-rotate-90',
          )}
        />
      </button>
      {open && (
        <div className="px-4 pb-3">
          {files.length === 0 ? (
            <p className="text-xs text-muted-foreground">暂无产物</p>
          ) : (
            <div className="flex flex-col gap-1">
              {files.map((f) => (
                <ArtifactItem
                  key={f.id}
                  file={f}
                  conversationId={conversationId}
                  canPreview={isPreviewActionEnabledForFile(f.actions, f.fileType, f.fileName)}
                  canOpenExternal={f.actions?.find((action) => action.type === 'open')?.enabled !== false}
                  previewable={isPreviewableFileType(f.fileType, f.fileName)}
                  onOpenExternal={onOpenExternal}
                />
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function ArtifactItem({
  file,
  conversationId,
  canPreview,
  canOpenExternal,
  previewable,
  onOpenExternal,
}: {
  file: GeneratedFile
  conversationId: string
  canPreview: boolean
  canOpenExternal: boolean
  previewable: boolean
  onOpenExternal?: (target: PreviewTarget) => void
}) {
  const target = useGeneratedFilePreviewStore((s) => s.target)
  const openPreview = useGeneratedFilePreviewStore((s) => s.openPreview)
  const active = target?.conversationId === conversationId && target.fileId === file.id
  const canPreviewInside = canPreview && previewable
  const canOpen = !canPreviewInside && canOpenExternal && Boolean(onOpenExternal)
  const enabled = canPreviewInside || canOpen
  const actionLabel = canPreviewInside ? '预览' : '打开'

  return (
    <button
      type="button"
      aria-label={`${actionLabel} ${file.fileName}`}
      disabled={!enabled}
      onClick={() => {
        const previewTarget = toPreviewTarget(file, conversationId)
        if (canPreviewInside) {
          openPreview(previewTarget)
          return
        }
        onOpenExternal?.(previewTarget)
      }}
      className={cn(
        'flex w-full items-center gap-2 rounded-md px-2 py-1 text-left hover:bg-muted/70',
        active && 'bg-muted',
        !enabled && 'cursor-not-allowed opacity-50 hover:bg-transparent',
      )}
    >
      <ArtifactFileIcon fileType={file.fileType} />
      <span className="min-w-0 flex-1 truncate text-xs text-foreground">
        {file.fileName}
      </span>
    </button>
  )
}

function ArtifactFileIcon({ fileType }: { fileType?: string }) {
  switch (fileType?.toLowerCase()) {
    case 'excel':
    case 'csv':
      return <FileSpreadsheet className="h-3.5 w-3.5 shrink-0 text-green-600" />
    case 'html':
    case 'pdf':
      return <FileText className="h-3.5 w-3.5 shrink-0 text-blue-500" />
    case 'png':
      return <Image className="h-3.5 w-3.5 shrink-0 text-purple-500" />
    default:
      return <File className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
  }
}
