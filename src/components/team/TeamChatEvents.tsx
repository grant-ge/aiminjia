import type { TeamEvent } from '@/types/team'
import { cn } from '@/lib/utils'
import { AgentAvatar } from './AgentAvatar'
import { getAgentIdentity, formatLeadDisplayName, isLeadName } from './agentIdentity'
import { formatClock, formatTimestampForGroup } from './formatters'

interface TeamChatEventsProps {
  events: TeamEvent[]
  /** When set, clicking a teammate's avatar fires this callback to open their detail view. */
  onDrillAgent?: (agentName: string) => void
}

/**
 * Render the chronological group-chat view of a team session.
 *
 * Layout follows the WeChat/iMessage pattern: lead messages on the right
 * (outbound, primary accent), teammate messages on the left (inbound,
 * per-agent palette slot). System events (TeamCreate / TeamDelete / spawn /
 * stop) are rendered as centered dividers with neutral muted styling.
 *
 * Date/time labels appear when there's a 5-minute gap between adjacent
 * events to avoid timestamp pollution in tight bursts.
 */
export function TeamChatEvents({ events, onDrillAgent }: TeamChatEventsProps) {
  if (events.length === 0) {
    return (
      <div className="flex h-full items-center justify-center px-6 py-12 text-sm text-muted-foreground">
        暂无消息
      </div>
    )
  }

  let lastTsForGroup: string | null = null

  return (
    <div className="flex flex-col gap-3 py-4">
      {events.map((event, idx) => {
        const groupLabel = formatTimestampForGroup(event.ts, lastTsForGroup)
        if (groupLabel) {
          lastTsForGroup = event.ts
        }
        return (
          <div key={idx} className="flex flex-col gap-1.5">
            {groupLabel && (
              <div className="flex justify-center">
                <span className="rounded-full bg-muted px-2.5 py-0.5 text-[11px] text-muted-foreground">
                  {groupLabel}
                </span>
              </div>
            )}
            <TeamEventRow event={event} onDrillAgent={onDrillAgent} />
          </div>
        )
      })}
    </div>
  )
}

interface TeamEventRowProps {
  event: TeamEvent
  onDrillAgent?: (agentName: string) => void
}

function TeamEventRow({ event, onDrillAgent }: TeamEventRowProps) {
  switch (event.kind) {
    case 'team_create':
      return <SystemDivider icon="●" label={`团队已创建${event.teamName ? ` · ${event.teamName}` : ''}`} ts={event.ts} />
    case 'team_delete':
      return <SystemDivider icon="○" label="团队已解散" ts={event.ts} />
    case 'agent_spawn':
      return <SystemDivider icon="＋" label={`${event.agentName} 加入团队`} ts={event.ts} />
    case 'agent_stop':
      return <SystemDivider icon="－" label={`${event.agentName} 已退出`} ts={event.ts} />
    case 'send_message':
      return <MessageBubble side="right" from={event.from} text={event.text} ts={event.ts} isError={event.isError} to={event.to} onDrillAgent={onDrillAgent} />
    case 'peer_message':
      return <MessageBubble side="left" from={event.from} text={event.text} ts={event.ts} isError={false} to={event.to} onDrillAgent={onDrillAgent} />
    default: {
      // Compile-time exhaustiveness check.
      const _exhaustive: never = event
      return _exhaustive
    }
  }
}

interface SystemDividerProps {
  icon: string
  label: string
  ts: string
}

function SystemDivider({ icon, label, ts }: SystemDividerProps) {
  return (
    <div className="flex items-center justify-center gap-2 text-[11px] text-muted-foreground">
      <span className="h-px flex-1 bg-border" />
      <span className="inline-flex items-center gap-1.5">
        <span aria-hidden>{icon}</span>
        <span>{label}</span>
        <span className="opacity-60">{formatClock(ts)}</span>
      </span>
      <span className="h-px flex-1 bg-border" />
    </div>
  )
}

interface MessageBubbleProps {
  side: 'left' | 'right'
  from: string
  to: string
  text: string
  ts: string
  isError: boolean
  onDrillAgent?: (agentName: string) => void
}

function MessageBubble({ side, from, to, text, ts, isError, onDrillAgent }: MessageBubbleProps) {
  const fromIdentity = getAgentIdentity(from)
  const displayFromName = formatLeadDisplayName(from)
  const displayToName = formatLeadDisplayName(to)
  const isDrillable = !isLeadName(from) && Boolean(onDrillAgent)

  const avatarNode = (
    <AgentAvatar
      name={from}
      size="md"
      className={cn(isDrillable && 'cursor-pointer transition-transform hover:scale-105')}
    />
  )
  const wrappedAvatar = isDrillable ? (
    <button type="button" onClick={() => onDrillAgent?.(from)} title={`查看 ${from} 的完整过程`}>
      {avatarNode}
    </button>
  ) : (
    avatarNode
  )

  return (
    <div className={cn('flex gap-2', side === 'right' && 'flex-row-reverse')}>
      {wrappedAvatar}
      <div className={cn('flex max-w-[78%] min-w-0 flex-col gap-0.5', side === 'right' && 'items-end')}>
        <div className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
          <span className="font-medium text-foreground">{displayFromName}</span>
          <span>→ {displayToName}</span>
          <span className="opacity-60">{formatClock(ts)}</span>
        </div>
        <div
          className={cn(
            'whitespace-pre-wrap break-words rounded-lg px-3 py-2 text-sm',
            isError
              ? 'border border-destructive/40 bg-destructive/10 text-destructive'
              : fromIdentity.bubbleClass,
          )}
        >
          {text || <span className="italic text-muted-foreground">（空消息）</span>}
          {isError && (
            <div className="mt-1 text-xs font-medium opacity-80">⚠ 发送失败</div>
          )}
        </div>
      </div>
    </div>
  )
}
