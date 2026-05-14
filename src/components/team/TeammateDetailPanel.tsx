import { useMemo, useState } from 'react'
import { ChevronDown, ChevronRight } from 'lucide-react'

import { AssistantMarkdown } from '@/components/chat-scene/AssistantMarkdown'
import { Button } from '@/components/ui/button'
import { useTeammateTranscript } from '@/hooks/useTeamOverview'

import { AgentAvatar } from './AgentAvatar'

interface TeammateDetailPanelProps {
  conversationId: string
  agentId: string
  agentName: string
  onBack: () => void
}

interface RawEntry {
  role?: string
  content?: unknown
  tool_calls?: Array<{ id?: string; name?: string; arguments?: unknown }>
  tool_call_id?: string
  tool_name?: string
  from?: string
}

interface ToolCallView {
  id: string
  name: string
  args: unknown
  result?: { content: unknown; tool_name?: string }
}

type Group =
  | { kind: 'system-reminder'; text: string }
  | { kind: 'incoming'; text: string; from: string | null; raw: unknown }
  | { kind: 'turn'; thought: string; toolCalls: ToolCallView[] }

function groupEntries(entries: unknown[]): Group[] {
  const list = entries.map((e) => (e ?? {}) as RawEntry)
  const resultsById = new Map<string, RawEntry>()
  for (const e of list) {
    if (e.role === 'tool' && typeof e.tool_call_id === 'string') {
      resultsById.set(e.tool_call_id, e)
    }
  }
  const groups: Group[] = []
  let sawSystemReminder = false
  for (const e of list) {
    if (e.role === 'tool') continue
    if (e.role === 'user') {
      const text = stringify(e.content)
      if (!sawSystemReminder && text.startsWith('<system-reminder>')) {
        groups.push({ kind: 'system-reminder', text })
        sawSystemReminder = true
      } else {
        const from = typeof e.from === 'string' && e.from.length > 0 ? e.from : null
        groups.push({ kind: 'incoming', text, from, raw: e.content })
      }
      continue
    }
    if (e.role === 'assistant') {
      const thought = stringify(e.content)
      const calls: ToolCallView[] = (e.tool_calls ?? []).map((tc) => {
        const id = tc.id ?? ''
        const result = id ? resultsById.get(id) : undefined
        return {
          id,
          name: tc.name ?? '?',
          args: tc.arguments,
          result: result
            ? { content: result.content, tool_name: result.tool_name }
            : undefined,
        }
      })
      groups.push({ kind: 'turn', thought, toolCalls: calls })
      continue
    }
    groups.push({ kind: 'incoming', text: stringify(e.content), from: null, raw: e.content })
  }
  return groups
}

export function TeammateDetailPanel({
  conversationId,
  agentId,
  agentName,
  onBack,
}: TeammateDetailPanelProps) {
  const { entries, loading } = useTeammateTranscript(conversationId, agentId)
  const groups = useMemo(() => groupEntries(entries ?? []), [entries])

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
        <div className="mx-auto w-full max-w-[736px]">
          {loading && (
            <div className="flex h-32 items-center justify-center text-sm text-muted-foreground">
              加载中…
            </div>
          )}
          {!loading && groups.length === 0 && (
            <div className="flex h-32 items-center justify-center text-sm text-muted-foreground">
              该成员没有可见记录
            </div>
          )}
          {!loading && groups.length > 0 && (
            <div className="flex flex-col gap-4">
              {groups.map((g, i) => (
                <GroupView key={i} group={g} />
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}

function GroupView({ group }: { group: Group }) {
  if (group.kind === 'system-reminder') return <SystemReminderBlock text={group.text} />
  if (group.kind === 'incoming')
    return <IncomingBubble text={group.text} from={group.from} raw={group.raw} />
  return <TurnBlock thought={group.thought} toolCalls={group.toolCalls} />
}

function SystemReminderBlock({ text }: { text: string }) {
  const [open, setOpen] = useState(false)
  return (
    <div className="rounded-md border border-border bg-muted/40">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-2 px-3 py-2 text-left text-xs text-muted-foreground hover:bg-muted/60"
      >
        {open ? <ChevronDown className="h-3.5 w-3.5" /> : <ChevronRight className="h-3.5 w-3.5" />}
        <span>角色说明</span>
        <span className="ml-auto opacity-60">system</span>
      </button>
      {open && (
        <pre className="whitespace-pre-wrap break-words border-t border-border px-3 py-2 text-[11px] leading-relaxed text-foreground/85">
          {text}
        </pre>
      )}
    </div>
  )
}

function IncomingBubble({ text, from, raw }: { text: string; from: string | null; raw: unknown }) {
  const header = from ? `接收到 ${from}` : '接收到'
  const parsed = parseMessage(text)
  return (
    <MessageCard
      header={header}
      tone="incoming"
      parsed={parsed}
      raw={raw}
    />
  )
}

function TurnBlock({ thought, toolCalls }: { thought: string; toolCalls: ToolCallView[] }) {
  const sendMessageCalls = toolCalls.filter((tc) => tc.name === 'SendMessage')
  const internalCalls = toolCalls.filter((tc) => tc.name !== 'SendMessage')
  return (
    <div className="flex flex-col gap-2">
      {thought && (
        <div className="text-sm text-foreground">
          <AssistantMarkdown text={thought} />
        </div>
      )}
      {internalCalls.length > 0 && (
        <div className="flex flex-col gap-1.5">
          {internalCalls.map((tc) => (
            <ToolChip key={tc.id} call={tc} />
          ))}
        </div>
      )}
      {sendMessageCalls.map((tc) => (
        <OutgoingBubble key={tc.id} call={tc} />
      ))}
    </div>
  )
}

function ToolChip({ call }: { call: ToolCallView }) {
  const [open, setOpen] = useState(false)
  const ok = call.result ? !isErrorResult(call.result.content) : undefined
  const summary = summarizeArgs(call.args)
  return (
    <div className="rounded-md border border-border bg-card">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-xs hover:bg-muted/60"
      >
        {open ? (
          <ChevronDown className="h-3.5 w-3.5 shrink-0" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5 shrink-0" />
        )}
        <span className="font-mono text-[11px] text-foreground">{call.name}</span>
        {summary && <span className="truncate text-muted-foreground">{summary}</span>}
        <span className="ml-auto shrink-0">
          {call.result == null ? (
            <span className="text-[10px] text-muted-foreground">—</span>
          ) : ok ? (
            <span className="text-[10px] text-emerald-600 dark:text-emerald-400">✓</span>
          ) : (
            <span className="text-[10px] text-destructive">✗</span>
          )}
        </span>
      </button>
      {open && (
        <div className="space-y-2 border-t border-border px-2.5 py-2">
          <div>
            <div className="mb-1 text-[10px] uppercase tracking-wide text-muted-foreground">args</div>
            <pre className="overflow-x-auto whitespace-pre-wrap break-all rounded bg-muted/40 px-2 py-1.5 font-mono text-[11px] leading-relaxed text-foreground/85">
              {prettyJson(call.args)}
            </pre>
          </div>
          {call.result && (
            <div>
              <div className="mb-1 text-[10px] uppercase tracking-wide text-muted-foreground">result</div>
              <pre className="overflow-x-auto whitespace-pre-wrap break-all rounded bg-muted/40 px-2 py-1.5 font-mono text-[11px] leading-relaxed text-foreground/85">
                {stringify(call.result.content)}
              </pre>
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function OutgoingBubble({ call }: { call: ToolCallView }) {
  const args = (call.args ?? {}) as { to?: unknown; message?: unknown }
  const to = typeof args.to === 'string' ? args.to : '?'
  const parsed = parseMessage(args.message)
  return (
    <MessageCard
      header={`发送给 ${to}`}
      tone="outgoing"
      parsed={parsed}
      raw={args.message}
    />
  )
}

/**
 * Parse a SendMessage `message` field (or any nested content payload) and
 * report back a structured result.  The renderer then decides whether to show
 * the parsed text, an "empty" placeholder, or a "could not parse" notice.
 */
interface ParsedMessage {
  text: string
  /** True when the original value was missing / blank after unwrap. */
  empty: boolean
  /** Set when the value was a string that *looked* like JSON but failed to
   *  yield extractable text — surfaces in the footer so debug is possible. */
  warning: string | null
}

function parseMessage(v: unknown): ParsedMessage {
  if (v == null) return { text: '', empty: true, warning: null }
  if (typeof v === 'string') {
    // Try one layer of "looks-like-JSON" unwrap so a stringified content block
    // (e.g. {"type":"text","content":"..."}) renders as prose.
    const trimmed = v.trim()
    if (trimmed.startsWith('{') && trimmed.includes('"type"')) {
      try {
        const parsed = JSON.parse(trimmed) as unknown
        const inner = stringify(parsed)
        if (inner.trim().length === 0) {
          return { text: '', empty: true, warning: 'JSON 内含 type=text 但 text/content 为空' }
        }
        return { text: inner, empty: false, warning: null }
      } catch {
        return { text: v, empty: false, warning: 'JSON-like 字符串解析失败，回退到原字符串' }
      }
    }
    return v.length === 0
      ? { text: '', empty: true, warning: null }
      : { text: v, empty: false, warning: null }
  }
  const out = stringify(v)
  if (out.trim().length === 0) return { text: '', empty: true, warning: null }
  // stringify fell through to JSON.stringify ⇒ unknown shape, flag it.
  if (out.startsWith('{') || out.startsWith('[')) {
    return { text: out, empty: false, warning: '未识别的结构，按 JSON 显示' }
  }
  return { text: out, empty: false, warning: null }
}

interface MessageCardProps {
  header: string
  tone: 'incoming' | 'outgoing'
  parsed: ParsedMessage
  raw: unknown
}

function MessageCard({ header, tone, parsed, raw }: MessageCardProps) {
  const [open, setOpen] = useState(false)
  const bodyClass =
    tone === 'incoming'
      ? 'border-border bg-muted/40'
      : 'border-primary/30 bg-primary/10'
  return (
    <div className={`w-fit max-w-[85%] overflow-hidden rounded-lg border ${bodyClass}`}>
      <div className="flex items-center gap-2 border-b border-current/15 bg-foreground/5 px-3 py-1.5 text-[11px] font-semibold uppercase tracking-wide text-foreground/80">
        <span>{header}</span>
        {parsed.warning && (
          <span
            className="rounded bg-amber-500/15 px-1.5 py-0.5 text-[10px] font-medium normal-case tracking-normal text-amber-700 dark:text-amber-300"
            title={parsed.warning}
          >
            解析提示
          </span>
        )}
      </div>
      <div className="px-3 py-2 text-sm">
        {parsed.empty ? (
          <span className="text-xs italic text-muted-foreground">（空消息）</span>
        ) : (
          <AssistantMarkdown text={parsed.text} />
        )}
      </div>
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-1.5 border-t border-current/15 bg-foreground/5 px-3 py-1.5 text-left text-[11px] font-semibold uppercase tracking-wide text-foreground/80 hover:bg-foreground/10"
      >
        {open ? (
          <ChevronDown className="h-3.5 w-3.5" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5" />
        )}
        <span>原始数据</span>
        {parsed.warning && (
          <span className="ml-auto text-[10px] font-medium normal-case tracking-normal text-amber-700 dark:text-amber-400">
            {parsed.warning}
          </span>
        )}
      </button>
      {open && (
        <pre className="overflow-x-auto whitespace-pre-wrap break-all border-t border-current/10 bg-card/60 px-3 py-1.5 font-mono text-[10px] leading-relaxed text-foreground/85">
          {prettyJson(raw)}
        </pre>
      )}
    </div>
  )
}

function summarizeArgs(args: unknown): string {
  if (args == null || typeof args !== 'object') return ''
  const a = args as Record<string, unknown>
  if (typeof a.file_path === 'string') return a.file_path
  if (typeof a.path === 'string') return a.path
  if (typeof a.command === 'string') return a.command
  if (typeof a.query === 'string') return a.query
  if (typeof a.url === 'string') return a.url
  if (typeof a.taskId === 'string') return a.taskId
  return ''
}

function isErrorResult(content: unknown): boolean {
  const s = typeof content === 'string' ? content : stringify(content)
  return /tool execution failed|error|fail/i.test(s)
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
  // Anthropic-style single content block: {type:"text", text:"..."} or
  // {type:"text", content:"..."}. Render the inner text directly.
  if (typeof v === 'object' && !Array.isArray(v)) {
    const obj = v as Record<string, unknown>
    if (obj.type === 'text') {
      if (typeof obj.text === 'string') return obj.text
      if (typeof obj.content === 'string') return obj.content
    }
  }
  // Array of content blocks (Anthropic message content): join the text-typed
  // ones, fall back to JSON for unknown blocks.
  if (Array.isArray(v)) {
    return v.map((b) => stringify(b)).join('\n')
  }
  try {
    return JSON.stringify(v)
  } catch {
    return String(v)
  }
}
