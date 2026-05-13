/**
 * TeamDrawer — 群聊右侧抽屉
 *
 * 在 ChatPage 右侧滑出，宽度 480px（全屏化时占满）。展示从 TeamView 派生
 * 出的事件流：
 * - 系统事件（团队创建/成员加入/任务创建·分派·完成）渲染为居中 pill
 * - 消息事件（lead → member / member → lead）渲染为左右气泡
 *
 * 设计强约束（与 PRD §5.3 对齐）：
 * - lead（AI小家）的发言**居左**——它是用户在群里的代理人
 * - 其他成员的发言**居右**
 * - 用户在 v1 群里不发言，抽屉底部固定提示"回 1v1 调度台"
 * - 工具调用不入流（解析器已过滤），消息气泡上不显示工具数（v1 简化）
 */
import { Maximize2, Minimize2, X, ArrowLeft } from 'lucide-react'
import { useMemo } from 'react'
import { useUiStore } from '@/stores/uiStore'
import type { TeamEvent, TeamRoster, TeamView } from '@/types/team'
import { AssistantMarkdown } from '@/components/chat-scene/AssistantMarkdown'

interface TeamDrawerProps {
  view: TeamView
}

export function TeamDrawer({ view }: TeamDrawerProps) {
  const open = useUiStore((s) => s.teamDrawerOpen)
  const expanded = useUiStore((s) => s.teamDrawerExpanded)
  const closeDrawer = useUiStore((s) => s.closeTeamDrawer)
  const setExpanded = useUiStore((s) => s.setTeamDrawerExpanded)

  if (!open) return null
  if (!view.roster.team_name) return null

  return (
    <aside
      data-testid="team-drawer"
      className={[
        'flex flex-col overflow-hidden border-l border-border bg-card transition-[width] duration-200',
        expanded ? 'w-full' : 'w-[480px] flex-shrink-0',
      ].join(' ')}
      aria-label="群聊视图"
    >
      <DrawerHeader
        teamName={view.roster.team_name}
        memberCount={view.roster.members.length + 1}
        taskTotal={view.roster.task_count_total}
        taskDone={view.roster.task_count_completed}
        expanded={expanded}
        onToggleExpand={() => setExpanded(!expanded)}
        onClose={closeDrawer}
      />
      <DrawerBody events={view.events} roster={view.roster} />
      <DrawerFooter onBack={closeDrawer} />
    </aside>
  )
}

function DrawerHeader({
  teamName,
  memberCount,
  taskTotal,
  taskDone,
  expanded,
  onToggleExpand,
  onClose,
}: {
  teamName: string
  memberCount: number
  taskTotal: number
  taskDone: number
  expanded: boolean
  onToggleExpand: () => void
  onClose: () => void
}) {
  return (
    <div className="flex items-center gap-3 border-b border-border bg-muted/40 px-4 py-3">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2 text-sm font-semibold">
          <span className="inline-block">👥</span>
          <span className="truncate">{teamName}</span>
          <span className="h-1.5 w-1.5 flex-shrink-0 rounded-full bg-emerald-500" aria-hidden />
        </div>
        <div className="text-xs text-muted-foreground">
          {memberCount} 人{taskTotal > 0 ? ` · 任务 ${taskDone}/${taskTotal}` : ''}
        </div>
      </div>
      <button
        type="button"
        onClick={onToggleExpand}
        className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
        aria-label={expanded ? '退出全屏' : '全屏'}
      >
        {expanded ? <Minimize2 className="h-3.5 w-3.5" /> : <Maximize2 className="h-3.5 w-3.5" />}
      </button>
      <button
        type="button"
        onClick={onClose}
        className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
        aria-label="关闭群聊"
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  )
}

function DrawerBody({ events, roster }: { events: TeamEvent[]; roster: TeamRoster }) {
  // 谁是 lead 由数据派生，不写死字符串：
  // 在 roster.members 里的 = 被 spawn 进群的 teammate；不在的 sender 就是 lead
  // （main agent 不在 teammates/*.meta.json 里，因此不会出现在 roster.members）。
  // peer-message 里 to 字段如果不在 members 里，也按 lead 处理。
  const memberNames = useMemo(
    () => new Set(roster.members.map((m) => m.name)),
    [roster.members],
  )
  const items = useMemo(() => events.map((ev, i) => ({ ev, key: `${ev.kind}-${i}-${ev.ts}` })), [events])
  return (
    <div className="flex-1 overflow-y-auto px-3 py-3">
      <div className="flex flex-col gap-2">
        {items.length === 0 ? (
          <div className="py-12 text-center text-sm text-muted-foreground">
            群刚创建好，还没有动静……
          </div>
        ) : (
          items.map(({ ev, key }) => <EventRow key={key} ev={ev} memberNames={memberNames} />)
        )}
      </div>
    </div>
  )
}

function EventRow({ ev, memberNames }: { ev: TeamEvent; memberNames: Set<string> }) {
  switch (ev.kind) {
    case 'team_created':
      return (
        <SystemPill tone="info" ts={ev.ts}>
          🆕 创建了群「{ev.team_name}」
          {ev.description ? ` · ${ev.description}` : ''}
        </SystemPill>
      )
    case 'member_joined':
      return (
        <SystemPill tone="info" ts={ev.ts}>
          👋 {ev.name} 加入群聊
        </SystemPill>
      )
    case 'task_created':
      return (
        <SystemPill tone="task" ts={ev.ts}>
          📋 {ev.subject}
        </SystemPill>
      )
    case 'task_updated':
      if (ev.status === 'completed') {
        return (
          <SystemPill tone="done" ts={ev.ts}>
            ✅ task#{ev.task_id} 已完成{ev.owner ? ` by ${ev.owner}` : ''}
          </SystemPill>
        )
      }
      if (ev.owner) {
        return (
          <SystemPill tone="info" ts={ev.ts}>
            → task#{ev.task_id} 分派给 {ev.owner}
          </SystemPill>
        )
      }
      if (ev.status) {
        return (
          <SystemPill tone="info" ts={ev.ts}>
            ⏳ task#{ev.task_id} → {ev.status}
          </SystemPill>
        )
      }
      return null
    case 'message_sent':
      return <ChatBubble ev={ev} memberNames={memberNames} />
  }
}

function SystemPill({
  children,
  tone,
  ts,
}: {
  children: React.ReactNode
  tone: 'info' | 'task' | 'done'
  ts: string
}) {
  const cls =
    tone === 'task'
      ? 'bg-amber-50 text-amber-800 dark:bg-amber-900/20 dark:text-amber-200'
      : tone === 'done'
        ? 'bg-emerald-50 text-emerald-800 dark:bg-emerald-900/20 dark:text-emerald-200'
        : 'bg-muted text-muted-foreground'
  return (
    <div className="flex justify-center py-0.5">
      <div
        className={[
          'inline-flex max-w-[90%] items-center gap-2 rounded-full px-3 py-1 text-xs',
          cls,
        ].join(' ')}
        title={formatTs(ts)}
      >
        <span>{children}</span>
        <span className="text-[10px] opacity-60">{formatTime(ts)}</span>
      </div>
    </div>
  )
}

function ChatBubble({
  ev,
  memberNames,
}: {
  ev: Extract<TeamEvent, { kind: 'message_sent' }>
  memberNames: Set<string>
}) {
  // lead 推断逻辑（不写死字符串）：
  // - sender 不在 roster.members 列表里 → 是 lead（main agent）
  // - sender 在 members 列表里 → 是被 spawn 的 teammate
  // 同样规则用在 to 字段上：to 不在 members 里 → 是 lead 的别名
  const isLead = !memberNames.has(ev.sender)
  const senderDisplay = ev.sender
  const toDisplay = ev.to
  const initial = senderDisplay.charAt(0).toUpperCase()
  const avatarBg = isLead ? 'bg-primary text-primary-foreground' : 'bg-emerald-600 text-white'
  const bubbleBg = isLead ? 'bg-card border border-border' : 'bg-amber-100/70 dark:bg-amber-900/30'

  return (
    <div
      className={[
        'flex max-w-[92%] items-start gap-2',
        isLead ? 'self-start' : 'flex-row-reverse self-end',
      ].join(' ')}
    >
      <div
        className={[
          'flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-full text-[11px] font-semibold',
          avatarBg,
        ].join(' ')}
      >
        {initial}
      </div>
      <div className="min-w-0">
        <div
          className={[
            'mb-1 flex items-baseline gap-1.5 text-[11px]',
            isLead ? '' : 'flex-row-reverse',
          ].join(' ')}
        >
          <span className="font-medium text-foreground">{senderDisplay}</span>
          <span className="text-muted-foreground">{formatTime(ev.ts)}</span>
        </div>
        {/*
          IM 群聊样式：每条消息开头永远是 `@对方` 蓝色高亮 + 消息正文。
          统一格式让"谁发给谁"一眼可见，与具体业务场景（辩论/需求讨论/调研）解耦。
          正文走 markdown 渲染，与主对话面板一致；不再用 whitespace-pre 渲染裸文本。
        */}
        <div
          className={[
            'rounded-lg px-3 py-2 text-[13px] leading-relaxed break-words',
            bubbleBg,
          ].join(' ')}
        >
          <div className="mb-1">
            <span className="font-semibold text-primary">@{toDisplay}</span>
          </div>
          <AssistantMarkdown text={ev.content} />
        </div>
      </div>
    </div>
  )
}

function DrawerFooter({ onBack }: { onBack: () => void }) {
  return (
    <div className="flex items-center gap-3 border-t border-border bg-muted/40 px-4 py-2.5 text-xs text-muted-foreground">
      <div className="flex-1 leading-snug">
        v1 群是只读视图。
        <br />
        想下指令？请回 1v1 调度台。
      </div>
      <button
        type="button"
        onClick={onBack}
        className="inline-flex items-center gap-1 rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground hover:opacity-90"
      >
        <ArrowLeft className="h-3 w-3" />
        回 1v1
      </button>
    </div>
  )
}

function formatTime(ts: string): string {
  try {
    const d = new Date(ts)
    return `${pad(d.getHours())}:${pad(d.getMinutes())}`
  } catch {
    return ''
  }
}

function formatTs(ts: string): string {
  try {
    return new Date(ts).toLocaleString()
  } catch {
    return ts
  }
}

function pad(n: number): string {
  return n.toString().padStart(2, '0')
}
