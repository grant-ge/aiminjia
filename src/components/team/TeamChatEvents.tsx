import { useState, type JSX } from 'react'
import type { TFunction } from 'i18next'
import { useTranslation } from 'react-i18next'

import type { TeamEvent } from '@/types/team'
import { cn } from '@/lib/utils'
import { AssistantMarkdown } from '@/components/chat-scene/AssistantMarkdown'
import { getExpertDisplayName } from '@/features/expert-teams/teams'
import type { ExpertTeam } from '@/features/expert-teams/teams'
import { AgentAvatar } from './AgentAvatar'
import { formatLeadDisplayName, isLeadName } from './agentIdentity'
import { formatClock, formatTimestampForGroup } from './formatters'
import { useTeamVisualContext } from './TeamVisualContext'
import { Button } from '@/components/ui/button'

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
  const renderItems = buildTeamChatRenderItems(events)
  if (renderItems.length === 0) {
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
      {renderItems.map((item, idx) => {
        const event = item.kind === 'event' ? item.event : item.anchor
        const groupLabel = formatTimestampForGroup(event.ts, lastTsForGroup)
        if (groupLabel) {
          lastTsForGroup = event.ts
        }
        const currentSpeaker = item.kind === 'event' ? speakerKey(event) : null
        const speakerChanged = currentSpeaker !== null && lastSpeaker !== null && currentSpeaker !== lastSpeaker
        if (currentSpeaker !== null) lastSpeaker = currentSpeaker
        return (
          <div
            key={`${item.kind}-${idx}`}
            className={cn(
              'flex flex-col gap-1.5',
              idx === 0 ? '' : speakerChanged ? 'mt-5' : 'mt-3',
            )}
          >
            {groupLabel && (
              <div className="flex justify-center">
                <span className="rounded-md bg-muted px-2.5 py-0.5 text-[11px] text-muted-foreground">
                  {groupLabel}
                </span>
              </div>
            )}
            {item.kind === 'event' ? (
              <TeamEventRow event={item.event} onDrillAgent={onDrillAgent} />
            ) : (
              <FacilitationNote item={item} />
            )}
          </div>
        )
      })}
    </div>
  )
}

type TeamChatRenderItem =
  | { kind: 'event'; event: TeamEvent }
  | {
      kind: 'facilitation'
      anchor: Extract<TeamEvent, { kind: 'send_message' }>
      category: FacilitationCategory
      count: number
      recipients: string[]
      text: string
      details: FacilitationDetail[]
    }

type FacilitationCategory = 'assignment' | 'cross_review' | 'debate' | 'instruction' | 'hidden_low_signal'

interface FacilitationDetail {
  to: string
  text: string
  ts: string
}

function buildTeamChatRenderItems(events: TeamEvent[]): TeamChatRenderItem[] {
  const items: TeamChatRenderItem[] = []
  let idx = 0
  while (idx < events.length) {
    const event = events[idx]
    if (!isLeadTextMessage(event)) {
      items.push({ kind: 'event', event })
      idx += 1
      continue
    }

    const category = classifyLeadMessage(event.text)
    if (category === 'hidden_low_signal') {
      let count = 1
      const details: FacilitationDetail[] = [facilitationDetail(event)]
      let next = idx + 1
      while (
        next < events.length &&
        isLeadTextMessage(events[next]) &&
        classifyLeadMessage((events[next] as Extract<TeamEvent, { kind: 'send_message' }>).text) ===
          'hidden_low_signal'
      ) {
        details.push(facilitationDetail(events[next] as Extract<TeamEvent, { kind: 'send_message' }>))
        count += 1
        next += 1
      }
      items.push({
        kind: 'facilitation',
        anchor: event,
        category,
        count,
        recipients: [],
        text: '',
        details,
      })
      idx = next
      continue
    }

    const normalized = normalizeLeadText(event.text)
    const recipients = [event.to]
    const details: FacilitationDetail[] = [facilitationDetail(event)]
    let count = 1
    let next = idx + 1
    while (
      next < events.length &&
      isLeadTextMessage(events[next]) &&
      normalizeLeadText((events[next] as Extract<TeamEvent, { kind: 'send_message' }>).text) ===
        normalized &&
      classifyLeadMessage((events[next] as Extract<TeamEvent, { kind: 'send_message' }>).text) ===
        category
    ) {
      const nextEvent = events[next] as Extract<TeamEvent, { kind: 'send_message' }>
      recipients.push(nextEvent.to)
      details.push(facilitationDetail(nextEvent))
      count += 1
      next += 1
    }

    if (count > 1 || category !== 'instruction') {
      items.push({
        kind: 'facilitation',
        anchor: event,
        category,
        count,
        recipients,
        text: event.text,
        details,
      })
    } else {
      items.push({ kind: 'event', event })
    }
    idx = next
  }
  return items
}

function facilitationDetail(event: Extract<TeamEvent, { kind: 'send_message' }>): FacilitationDetail {
  return {
    to: event.to,
    text: event.text,
    ts: event.ts,
  }
}

function isLeadTextMessage(
  event: TeamEvent,
): event is Extract<TeamEvent, { kind: 'send_message' }> {
  return event.kind === 'send_message' && event.variant === 'text' && isLeadName(event.from)
}

function normalizeLeadText(text: string): string {
  return text.replace(/\s+/g, ' ').trim()
}

function classifyLeadMessage(text: string): FacilitationCategory {
  const normalized = normalizeLeadText(text)
  if (/交叉点评|互相点评|第二轮|其他三位|核心观点摘要|阅后点评|回应、补充或质疑|质询|反驳|最后一轮/.test(normalized)) {
    return 'cross_review'
  }
  if (/正方|反方|观察员|辩手|论点|陈词/.test(normalized)) {
    return 'debate'
  }
  if (/欢迎加入|自我介绍|开场热身|圆桌讨论正式开始|各自给出|第一轮|首轮|发表你的观点|直接发表|请就议题|请开始|请分享/.test(normalized)) {
    return 'assignment'
  }
  if (
    /收到|已记录|正在等待|保持等待|尚未提交|尚未发言|其他成员已就位|请尽快|准备好了吗/.test(
      normalized,
    )
  ) {
    return 'hidden_low_signal'
  }
  return 'instruction'
}

/** 区分发言人，用于决定相邻消息之间是否加大间隔。系统事件返回 null（不影响发言人切换判断）。 */
function speakerKey(event: TeamEvent): string | null {
  if (event.kind === 'send_message' || event.kind === 'peer_message') {
    return event.from
  }
  return null
}

function formatAgentDisplayName(
  teamVisual: ExpertTeam | null,
  agentName: string,
): string {
  if (isLeadName(agentName)) return formatLeadDisplayName(agentName)
  return getExpertDisplayName(teamVisual, agentName)
}

interface TeamEventRowProps {
  event: TeamEvent
  onDrillAgent?: (agentName: string) => void
}

function TeamEventRow({ event, onDrillAgent }: TeamEventRowProps) {
  const { t } = useTranslation()
  const teamVisual = useTeamVisualContext()
  switch (event.kind) {
    case 'team_create':
      return (
        <SystemDivider
          icon="●"
          label={
            event.teamName || teamVisual?.name
              ? t('team.chat.lifecycle.teamCreatedWithName', { teamName: teamVisual?.name ?? event.teamName })
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
          label={t('team.chat.lifecycle.agentJoined', {
            agentName: formatAgentDisplayName(teamVisual, event.agentName),
          })}
          ts={event.ts}
        />
      )
    case 'agent_stop':
      return (
        <SystemDivider
          icon="－"
          label={t('team.chat.lifecycle.agentLeft', {
            agentName: formatAgentDisplayName(teamVisual, event.agentName),
          })}
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
        }, teamVisual)
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
function renderProtocolDivider(
  t: TFunction,
  input: ProtocolDividerInput,
  teamVisual: ExpertTeam | null,
): JSX.Element | null {
  const fromDisplay = formatAgentDisplayName(teamVisual, input.from)
  const toDisplay = formatAgentDisplayName(teamVisual, input.to)
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
  const markerClass = systemMarkerClass(icon)
  return (
    <div className="flex justify-center px-8 py-0.5">
      <span className="inline-flex max-w-full items-center gap-1.5 rounded-full bg-muted/45 px-2.5 py-1 text-[11px] leading-4 text-muted-foreground">
        <span aria-hidden className={cn('h-1.5 w-1.5 shrink-0 rounded-full', markerClass)} />
        <span aria-hidden className="sr-only">{icon}</span>
        <span className="min-w-0">{label}</span>
        <span className="shrink-0 text-muted-foreground/55">{formatClock(ts)}</span>
      </span>
    </div>
  )
}

function systemMarkerClass(icon: string): string {
  switch (icon) {
    case '✓':
      return 'bg-primary/70'
    case '✗':
      return 'bg-destructive/70'
    case '⊙':
    case '≪':
      return 'bg-primary/45'
    default:
      return 'bg-muted-foreground/55'
  }
}

function FacilitationNote({ item }: { item: Extract<TeamChatRenderItem, { kind: 'facilitation' }> }) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(false)
  const teamVisual = useTeamVisualContext()
  const recipientNames = item.recipients
    .map((name) => formatAgentDisplayName(teamVisual, name))
    .join('、')
  const label =
    item.category === 'hidden_low_signal'
      ? t('team.chat.facilitation.hiddenLowSignal', { count: item.count })
      : t(`team.chat.facilitation.${item.category}`, {
          count: item.count,
          recipients: recipientNames,
        })
  if (item.category !== 'hidden_low_signal') {
    return (
      <div className="flex flex-col items-end gap-1 px-2">
        <div className="flex flex-row-reverse items-center gap-2 text-[11px] text-muted-foreground">
          <AgentAvatar name="team-lead" size="md" />
          <div className="flex flex-col items-end">
            <div className="flex flex-wrap items-center justify-end gap-1.5">
              <span>
                <span className="font-medium text-foreground">{formatLeadDisplayName('team-lead')}</span>
                {recipientNames ? ` → ${recipientNames}` : null}
              </span>
            </div>
            <div className="flex items-center gap-1.5">
              <span className="rounded-md bg-muted px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground">
                {label}
              </span>
              <span className="opacity-60">{formatClock(item.anchor.ts)}</span>
            </div>
          </div>
        </div>
        <div className="w-fit max-w-[85%] break-words rounded-md border border-border bg-card px-3 py-2 text-sm leading-6 text-foreground shadow-[var(--shadow-card)]">
          {item.text ? (
            <AssistantMarkdown text={item.text} />
          ) : (
            <span className="italic text-muted-foreground">{t('team.chat.emptyText')}</span>
          )}
        </div>
      </div>
    )
  }
  const actionLabel = expanded ? t('team.chat.facilitation.collapse') : t('team.chat.facilitation.expand')
  return (
    <div className="flex justify-end px-2">
      <div
        className={cn(
          'max-w-[88%] overflow-hidden rounded-md border text-xs leading-5 text-muted-foreground',
          item.category === 'hidden_low_signal'
            ? 'border-primary/20 bg-primary/5'
            : 'border-border bg-muted/35',
        )}
      >
        <button
          type="button"
          className="flex w-full items-start justify-between gap-3 px-3 py-2 text-left transition-colors hover:bg-muted/45"
          onClick={() => setExpanded((value) => !value)}
          aria-expanded={expanded}
          aria-label={`${actionLabel} ${label}`}
        >
          <span className="min-w-0">
            <span className="flex items-center gap-1.5 font-medium text-foreground/80">
              <span
                aria-hidden
                className={cn(
                  'h-1.5 w-1.5 shrink-0 rounded-full',
                  item.category === 'hidden_low_signal' ? 'bg-primary/70' : 'bg-muted-foreground/55',
                )}
              />
              <span>{label}</span>
            </span>
            {item.text ? <span className="mt-1 line-clamp-3 block whitespace-pre-wrap">{item.text}</span> : null}
          </span>
          <span className="shrink-0 rounded-md border border-border bg-card px-1.5 py-0.5 text-[11px] text-muted-foreground">
            {actionLabel}
          </span>
        </button>
        {expanded && (
          <div className="border-t border-border/70 bg-card/70 px-3 py-2">
            <div className="flex flex-col gap-2">
              {item.details.map((detail, index) => (
                <div key={`${detail.to}-${detail.ts}-${index}`} className="rounded-md bg-muted/30 px-2.5 py-2">
                  <div className="mb-1 flex items-center justify-between gap-2 text-[11px] text-muted-foreground">
                    <span>{formatLeadDisplayName('team-lead')} → {formatAgentDisplayName(teamVisual, detail.to)}</span>
                    <span className="shrink-0 opacity-70">{formatClock(detail.ts)}</span>
                  </div>
                  {detail.text ? (
                    <AssistantMarkdown text={detail.text} />
                  ) : (
                    <span className="italic text-muted-foreground">{t('team.chat.emptyText')}</span>
                  )}
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
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
  const teamVisual = useTeamVisualContext()
  const displayFromName = formatAgentDisplayName(teamVisual, from)
  const displayToName = formatAgentDisplayName(teamVisual, to)
  const isDrillable = !isLeadName(from) && Boolean(onDrillAgent)

  const avatarNode = (
    <AgentAvatar
      name={from}
      size="md"
      className={cn(isDrillable && 'cursor-pointer transition-transform hover:scale-105')}
    />
  )
  const wrappedAvatar = isDrillable ? (
    <Button unstyled
      type="button"
      onClick={() => onDrillAgent?.(from)}
      title={t('team.process.viewMemberFullProcess', { name: from })}
    >
      {avatarNode}
    </Button>
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
          'w-fit max-w-[85%] break-words rounded-md px-3 py-2 text-sm leading-6 shadow-[var(--shadow-card)]',
          isError
            ? 'border border-destructive/40 bg-destructive/10 text-destructive'
            : 'border border-border bg-card text-foreground',
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
