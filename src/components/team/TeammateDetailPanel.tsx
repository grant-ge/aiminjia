import { useTeammateTranscript } from '@/hooks/useTeamOverview'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

import { AgentAvatar } from './AgentAvatar'

interface TeammateDetailPanelProps {
  conversationId: string
  agentId: string
  agentName: string
  onBack: () => void
}

/**
 * Show one teammate's complete internal transcript. The data shape mirrors
 * the on-disk jsonl: each entry is `{role, content, tool_calls?, tool_call_id?, tool_name?}`.
 *
 * Rendering is intentionally minimal — this is a developer-style "what was
 * the agent thinking" view, not the polished chat surface. Code-style font
 * for tool calls keeps the visual contrast with the main chat view.
 */
export function TeammateDetailPanel({
  conversationId,
  agentId,
  agentName,
  onBack,
}: TeammateDetailPanelProps) {
  const { entries, loading } = useTeammateTranscript(conversationId, agentId)

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b border-border px-4 py-3">
        <Button variant="ghost" size="sm" onClick={onBack} className="h-7 px-2 text-xs">
          ← 返回
        </Button>
        <AgentAvatar name={agentName} size="md" />
        <div className="flex min-w-0 flex-col">
          <span className="truncate text-sm font-medium text-foreground">{agentName}</span>
          <span className="text-[11px] text-muted-foreground">完整内部过程</span>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto px-4 py-4">
        {loading && (
          <div className="flex h-32 items-center justify-center text-sm text-muted-foreground">
            加载中…
          </div>
        )}
        {!loading && entries && entries.length === 0 && (
          <div className="flex h-32 items-center justify-center text-sm text-muted-foreground">
            该成员没有可见记录
          </div>
        )}
        {!loading && entries && entries.length > 0 && (
          <div className="flex flex-col gap-3">
            {entries.map((entry, idx) => (
              <TranscriptEntryView key={idx} entry={entry} />
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

interface TranscriptEntry {
  role?: string
  content?: unknown
  tool_calls?: Array<{ id?: string; name?: string; arguments?: unknown }>
  tool_call_id?: string
  tool_name?: string
}

function TranscriptEntryView({ entry }: { entry: unknown }) {
  const e = (entry ?? {}) as TranscriptEntry
  const role = e.role ?? 'unknown'

  if (role === 'assistant' && Array.isArray(e.tool_calls) && e.tool_calls.length > 0) {
    return (
      <div className="space-y-1.5">
        {typeof e.content === 'string' && e.content && (
          <RoleBlock role={role} text={e.content} />
        )}
        {e.tool_calls.map((tc, i) => (
          <ToolCallBlock key={tc.id ?? i} name={tc.name ?? '?'} args={tc.arguments} />
        ))}
      </div>
    )
  }

  if (role === 'tool') {
    return <ToolResultBlock name={e.tool_name ?? '?'} content={e.content} />
  }

  // user or assistant text only
  return <RoleBlock role={role} text={stringify(e.content)} />
}

function RoleBlock({ role, text }: { role: string; text: string }) {
  const isUser = role === 'user'
  const isAssistant = role === 'assistant'
  const label = isUser ? '输入' : isAssistant ? '回应' : role
  return (
    <div
      className={cn(
        'rounded-md border px-3 py-2',
        isUser
          ? 'border-muted-foreground/20 bg-muted/40'
          : 'border-primary/20 bg-primary/5',
      )}
    >
      <div className="mb-1 text-[10px] uppercase tracking-wide text-muted-foreground">
        {label}
      </div>
      <div className="whitespace-pre-wrap break-words text-xs leading-relaxed text-foreground">
        {text}
      </div>
    </div>
  )
}

function ToolCallBlock({ name, args }: { name: string; args: unknown }) {
  return (
    <div className="rounded-md border border-blue-500/20 bg-blue-500/5 px-3 py-2">
      <div className="mb-1 flex items-center gap-2 text-[10px] uppercase tracking-wide text-muted-foreground">
        <span>调用工具</span>
        <span className="rounded bg-blue-500/15 px-1.5 py-0.5 font-mono text-[10px] text-blue-700 dark:text-blue-300">
          {name}
        </span>
      </div>
      <pre className="overflow-x-auto whitespace-pre-wrap break-all font-mono text-[11px] leading-relaxed text-foreground/85">
        {prettyJson(args)}
      </pre>
    </div>
  )
}

function ToolResultBlock({ name, content }: { name: string; content: unknown }) {
  return (
    <div className="rounded-md border border-emerald-500/20 bg-emerald-500/5 px-3 py-2">
      <div className="mb-1 flex items-center gap-2 text-[10px] uppercase tracking-wide text-muted-foreground">
        <span>工具结果</span>
        <span className="rounded bg-emerald-500/15 px-1.5 py-0.5 font-mono text-[10px] text-emerald-700 dark:text-emerald-300">
          {name}
        </span>
      </div>
      <pre className="overflow-x-auto whitespace-pre-wrap break-all font-mono text-[11px] leading-relaxed text-foreground/85">
        {stringify(content)}
      </pre>
    </div>
  )
}

function prettyJson(v: unknown): string {
  if (v == null) return ''
  try {
    return JSON.stringify(v, null, 2)
  } catch {
    return String(v)
  }
}

function stringify(v: unknown): string {
  if (typeof v === 'string') return v
  if (v == null) return ''
  try {
    return JSON.stringify(v)
  } catch {
    return String(v)
  }
}
