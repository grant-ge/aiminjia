import type { JSX } from 'react'
import type { TFunction } from 'i18next'
import { useTranslation } from 'react-i18next'

import type { TeamEvent } from '@/types/team'
import { cn } from '@/lib/utils'
import { AssistantMarkdown } from '@/components/chat-scene/AssistantMarkdown'
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
  const { t } = useTranslation()
  if (events.length === 0) {
    return (
      <div className="flex h-full items-center justify-center px-6 py-12 text-sm text-muted-foreground">
        {t('team.chat.empty')}
      </div>
    )
  }

  let lastTsForGroup: string | null = null
  let lastSpeaker: string | null = null

  return (
    <div className="flex flex-col py-4">
      {events.map((event, idx) => {
        const groupLabel = formatTimestampForGroup(event.ts, lastTsForGroup)
        if (groupLabel) {
          lastTsForGroup = event.ts
        }
        const currentSpeaker = speakerKey(event)
        const speakerChanged = currentSpeaker !== null && lastSpeaker !== null && currentSpeaker !== lastSpeaker
        if (currentSpeaker !== null) lastSpeaker = currentSpeaker
        return (
          <div
            key={idx}
            className={cn(
              'flex flex-col gap-1.5',
              idx === 0 ? '' : speakerChanged ? 'mt-5' : 'mt-3',
            )}
          >
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

/** 区分发言人，用于决定相邻消息之间是否加大间隔。系统事件返回 null（不影响发言人切换判断）。 */
function speakerKey(event: TeamEvent): string | null {
  if (event.kind === 'send_message' || event.kind === 'peer_message') {
    return event.from
  }
  return null
}

interface TeamEventRowProps {
  event: TeamEvent
  onDrillAgent?: (agentName: string) => void
}

function TeamEventRow({ event, onDrillAgent }: TeamEventRowProps) {
  const { t } = useTranslation()
  switch (event.kind) {
    case 'team_create':
      return (
        <SystemDivider
          icon="●"
          label={
            event.teamName
              ? t('team.chat.lifecycle.teamCreatedWithName', { teamName: event.teamName })
              : t('team.chat.lifecycle.teamCreated')
          }
          ts={event.ts}
        />
      )
    case 'team_delete':
      return <SystemDivider icon="○" label={t('team.chat.lifecycle.teamDeleted')} ts={event.ts} />
    case 'agent_spawn':
      return (
        <SystemDivider
          icon="＋"
          label={t('team.chat.lifecycle.agentJoined', { agentName: formatLeadDisplayName(event.agentName) })}
          ts={event.ts}
        />
      )
    case 'agent_stop':
      return (
        <SystemDivider
          icon="－"
          label={t('team.chat.lifecycle.agentLeft', { agentName: formatLeadDisplayName(event.agentName) })}
          ts={event.ts}
        />
      )
    case 'send_message': {
      // X 方案：variant !== 'text' 时按协议握手类型走 SystemDivider；
      // text variant 才渲染对话气泡。这避免空 text + 协议字段的握手消息撞
      // 兜底"（空消息）"分支，跟 team_create/agent_spawn 视觉对称。
      if (event.variant !== 'text') {
        const divider = renderProtocolDivider(t, {
          variant: event.variant,
          from: event.from,
          to: event.to,
          ts: event.ts,
          approve: event.approve,
          reason: event.reason,
          feedback: event.feedback,
        })
        if (divider) return divider
      }
      return (
        <MessageBubble
          side={isLeadName(event.from) ? 'right' : 'left'}
          from={event.from}
          text={event.text}
          ts={event.ts}
          isError={event.isError}
          to={event.to}
          onDrillAgent={onDrillAgent}
        />
      )
    }
    case 'peer_message':
      return (
        <MessageBubble
          side={isLeadName(event.from) ? 'right' : 'left'}
          from={event.from}
          text={event.text}
          ts={event.ts}
          isError={false}
          to={event.to}
          onDrillAgent={onDrillAgent}
        />
      )
    default: {
      // Compile-time exhaustiveness check.
      const _exhaustive: never = event
      return _exhaustive
    }
  }
}

interface ProtocolDividerInput {
  variant: string
  from: string
  to: string
  ts: string
  approve?: boolean
  reason?: string
  feedback?: string
}

/**
 * 把 4 个协议握手 variant 映射到 SystemDivider 输入。返回 null 表示遇到
 * 未识别 variant —— 调用方会回退到 MessageBubble。
 */
function renderProtocolDivider(t: TFunction, input: ProtocolDividerInput): JSX.Element | null {
  const fromDisplay = formatLeadDisplayName(input.from)
  const toDisplay = formatLeadDisplayName(input.to)
  switch (input.variant) {
    case 'shutdown_request': {
      const label = input.reason
        ? t('team.chat.protocol.shutdownRequestWithReason', {
            from: fromDisplay,
            to: toDisplay,
            reason: input.reason,
          })
        : t('team.chat.protocol.shutdownRequest', { from: fromDisplay, to: toDisplay })
      return <SystemDivider icon="⊙" label={label} ts={input.ts} />
    }
    case 'shutdown_response': {
      if (input.approve === false) {
        const label = input.reason
          ? t('team.chat.protocol.shutdownRejectWithReason', {
              from: fromDisplay,
              reason: input.reason,
            })
          : t('team.chat.protocol.shutdownReject', { from: fromDisplay })
        return <SystemDivider icon="✗" label={label} ts={input.ts} />
      }
      // approve === true 或 approve 缺失，按"同意"渲染（spec §5）。
      return (
        <SystemDivider
          icon="✓"
          label={t('team.chat.protocol.shutdownApprove', { from: fromDisplay })}
          ts={input.ts}
        />
      )
    }
    case 'plan_approval_request':
      return (
        <SystemDivider
          icon="≪"
          label={t('team.chat.protocol.planApprovalRequest', { from: fromDisplay, to: toDisplay })}
          ts={input.ts}
        />
      )
    case 'plan_approval_response': {
      if (input.approve === false) {
        const label = input.feedback
          ? t('team.chat.protocol.planReject', { from: fromDisplay, feedback: input.feedback })
          : t('team.chat.protocol.planRejectNoFeedback', { from: fromDisplay })
        return <SystemDivider icon="✗" label={label} ts={input.ts} />
      }
      return (
        <SystemDivider
          icon="✓"
          label={t('team.chat.protocol.planApprove', { from: fromDisplay })}
          ts={input.ts}
        />
      )
    }
    default:
      return null
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
  const { t } = useTranslation()
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
    <div className={cn('flex flex-col gap-1', side === 'right' ? 'items-end' : 'items-start')}>
      <div
        className={cn(
          'flex items-center gap-2 text-[11px] text-muted-foreground',
          side === 'right' && 'flex-row-reverse',
        )}
      >
        {wrappedAvatar}
        <div className={cn('flex flex-col', side === 'right' && 'items-end')}>
          <div className="flex items-center gap-1.5">
            <span className="font-medium text-foreground">{displayFromName}</span>
            <span>→ {displayToName}</span>
          </div>
          <span className="opacity-60">{formatClock(ts)}</span>
        </div>
      </div>
      <div
        className={cn(
          'w-fit max-w-[85%] break-words rounded-lg px-3 py-2 text-sm',
          isError
            ? 'border border-destructive/40 bg-destructive/10 text-destructive'
            : fromIdentity.bubbleClass,
        )}
      >
        {text ? (
          <AssistantMarkdown text={text} />
        ) : (
          <span className="italic text-muted-foreground">{t('team.chat.emptyText')}</span>
        )}
        {isError && (
          <div className="mt-1 text-xs font-medium opacity-80">{t('team.chat.sendFailed')}</div>
        )}
      </div>
    </div>
  )
}
