import { useMemo, useState, useEffect } from 'react'
import {
  ChevronDown,
  ChevronRight,
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

import { cn } from '@/lib/utils'
import { useChatStore } from '@/stores/chatStore'
import type { ConversationTaskState } from '@/stores/streamingStore'
import type { GeneratedFile } from '@/types/message'

// ─── RightPanel root ──────────────────────────────────────────────────────────

interface RightPanelProps {
  conversationId: string
}

export function RightPanel({ conversationId }: RightPanelProps) {
  return (
    <div className="flex w-[260px] shrink-0 flex-col overflow-y-auto border-l border-border bg-background">
      <div className="px-4 py-4">
        <h2 className="text-[15px] font-semibold text-foreground">任务监控</h2>
      </div>
      <TaskSection conversationId={conversationId} />
      <ArtifactSection conversationId={conversationId} />
      <SkillMcpSection />
    </div>
  )
}

// ─── TaskSection ──────────────────────────────────────────────────────────────

function TaskSection({ conversationId }: { conversationId: string }) {
  const tasks = useChatStore((s) => s.taskStates[conversationId] ?? [])
  const hasRunning = tasks.some((t) => t.status === 'running')
  const [open, setOpen] = useState(true)

  useEffect(() => {
    if (hasRunning) setOpen(true)
  }, [hasRunning])

  return (
    <div className="border-b border-border">
      <button
        type="button"
        onClick={() => !hasRunning && setOpen((v) => !v)}
        className="flex w-full items-center justify-between px-4 py-3 text-left"
      >
        <span className="text-[13px] font-semibold text-foreground">待办</span>
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
            <p className="text-[12px] text-muted-foreground">暂无待办</p>
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
            'text-[12px] font-medium leading-tight',
            task.status === 'completed'
              ? 'text-muted-foreground line-through'
              : 'text-foreground',
          )}
        >
          {task.subject}
        </span>
        {task.status === 'running' && task.activeForm && (
          <span className="text-[11px] text-primary">{task.activeForm}</span>
        )}
      </div>
    </div>
  )
}

function TaskStatusIcon({ status }: { status: string }) {
  switch (status) {
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

// ─── ArtifactSection ──────────────────────────────────────────────────────────

function ArtifactSection({ conversationId: _conversationId }: { conversationId: string }) {
  const messages = useChatStore((s) => s.messages)
  const [open, setOpen] = useState(true)

  const files = useMemo(() => {
    const seen = new Set<string>()
    const result: GeneratedFile[] = []
    for (const msg of messages) {
      for (const f of msg.content.generatedFiles ?? []) {
        if (!seen.has(f.id) && f.isLatest) {
          seen.add(f.id)
          result.push(f)
        }
      }
    }
    return result
  }, [messages])

  return (
    <div className="border-b border-border">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center justify-between px-4 py-3 text-left"
      >
        <span className="text-[13px] font-semibold text-foreground">产物</span>
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
            <p className="text-[12px] text-muted-foreground">暂无产物</p>
          ) : (
            <div className="flex flex-col gap-1">
              {files.map((f) => (
                <ArtifactItem key={f.id} file={f} />
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function ArtifactItem({ file }: { file: GeneratedFile }) {
  return (
    <div className="flex items-center gap-2 py-1">
      <ArtifactFileIcon fileType={file.fileType} />
      <span className="min-w-0 flex-1 truncate text-[12px] text-foreground">
        {file.fileName}
      </span>
    </div>
  )
}

function ArtifactFileIcon({ fileType }: { fileType: string }) {
  switch (fileType) {
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

// ─── SkillMcpSection ──────────────────────────────────────────────────────────

function SkillMcpSection() {
  return (
    <div className="border-b border-border">
      <button
        type="button"
        className="flex w-full items-center justify-between px-4 py-3 text-left"
        onClick={() => {
          // TODO: 跳转到技能与 MCP 配置页（占位）
        }}
      >
        <span className="text-[13px] font-semibold text-foreground">技能与 MCP</span>
        <ChevronRight className="h-4 w-4 text-muted-foreground" />
      </button>
    </div>
  )
}
