import { useEffect, useMemo, useRef, useState, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { ArrowDown, X } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { useSettingsStore } from '@/stores/settingsStore'
import type { TeamOverview, TeamSession } from '@/types/team'
import { useConversationTeamState, useTeamStore } from '@/stores/teamStore'
import { getExpertDisplayName } from '@/features/expert-teams/teams'

import { AgentAvatar } from './AgentAvatar'
import { TeamChatEvents } from './TeamChatEvents'
import { TeammateDetailPanel } from './TeammateDetailPanel'
import { formatDuration, formatShortDateTime } from './formatters'
import { isLeadName } from './agentIdentity'
import { useTeamVisualContext } from './TeamVisualContext'

interface TeamChatDrawerProps {
  conversationId: string
  overview: TeamOverview | null
}

/**
 * Inline right-side panel rendering the team session timeline. Mounted as a
 * sibling of the chat column in ChatPage / ChannelPage so the panel spans
 * full chat height (input row + scroll region together).
 *
 * Drill-down: click a teammate avatar to swap the body to TeammateDetailPanel
 * for that agent.
 */
export function TeamChatDrawer({ conversationId, overview }: TeamChatDrawerProps) {
  const state = useConversationTeamState(conversationId)
  const closeDrawer = useTeamStore((s) => s.closeDrawer)
  const setDrillAgent = useTeamStore((s) => s.setDrillAgent)

  const drillAgent = useMemo(() => {
    if (!state.drillAgentId || !overview) return null
    for (const session of overview.teams) {
      const found = session.members.find((m) => m.agentId === state.drillAgentId)
      if (found) return found
    }
    return null
  }, [state.drillAgentId, overview])

  if (!state.drawerOpen) return null

  return (
    <aside
      data-testid="team-split-panel"
      className="flex h-full min-w-0 flex-1 flex-col border-l border-border bg-background"
    >
      {drillAgent ? (
        <TeammateDetailPanel
          conversationId={conversationId}
          agentId={drillAgent.agentId}
          agentName={drillAgent.agentName}
          onBack={() => setDrillAgent(conversationId, null)}
        />
      ) : (
        <DrawerOverview
          conversationId={conversationId}
          overview={overview}
          onDrill={(agentId) => setDrillAgent(conversationId, agentId)}
          onClose={() => closeDrawer(conversationId, true)}
        />
      )}
    </aside>
  )
}

interface DrawerOverviewProps {
  conversationId: string
  overview: TeamOverview | null
  onDrill: (agentId: string) => void
  onClose: () => void
}

function DrawerOverview({ conversationId, overview, onDrill, onClose }: DrawerOverviewProps) {
  const { t } = useTranslation()
  const focusedTeamId = useConversationTeamState(conversationId).focusedTeamId
  const clearFocusedTeam = useTeamStore((s) => s.clearFocusedTeam)
  const scrollRef = useRef<HTMLDivElement>(null)
  const contentRef = useRef<HTMLDivElement>(null)
  const userScrolledUp = useRef(false)
  const userIntentRef = useRef(false)
  const userIntentTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const [showJumpToBottom, setShowJumpToBottom] = useState(false)

  const markUserIntent = useCallback(() => {
    userIntentRef.current = true
    if (userIntentTimerRef.current) clearTimeout(userIntentTimerRef.current)
    userIntentTimerRef.current = setTimeout(() => {
      userIntentRef.current = false
    }, 800)
  }, [])

  useEffect(() => {
    return () => {
      if (userIntentTimerRef.current) clearTimeout(userIntentTimerRef.current)
    }
  }, [])

  const handleScroll = useCallback(() => {
    const el = scrollRef.current
    if (!el) return
    if (!userIntentRef.current) return
    const nextScrolledUp = el.scrollHeight - el.scrollTop - el.clientHeight > 100
    userScrolledUp.current = nextScrolledUp
    setShowJumpToBottom(nextScrolledUp)
  }, [])

  const jumpToBottom = useCallback(() => {
    userScrolledUp.current = false
    setShowJumpToBottom(false)
    const el = scrollRef.current
    if (el) el.scrollTo({ top: el.scrollHeight, behavior: 'smooth' })
  }, [])

  // Auto-scroll the panel to the bottom whenever new events land, unless the
  // user has pulled up. ResizeObserver fires when markdown finishes laying out
  // (delayed code highlighting, image load, etc) so the bottom keeps tracking.
  useEffect(() => {
    const content = contentRef.current
    if (!content || typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(() => {
      if (userScrolledUp.current) return
      const el = scrollRef.current
      if (el) el.scrollTop = el.scrollHeight
    })
    observer.observe(content)
    return () => observer.disconnect()
  }, [])

  // focusedTeamId：点击主聊天里的 TeamProgressBlock 卡片会通过 openDrawer(convId,
  // teamId) 把焦点写进 store；本 effect 等抽屉首次 render 出对应 section 之后
  // scrollIntoView 一次，立刻 clear 焦点防止后续滚动被反复抢回。同时把
  // userScrolledUp 置 true，让 ResizeObserver 不再自动追到底部（否则 markdown
  // 排版完成后 scroll 又会跳到最新 team）。
  useEffect(() => {
    if (!focusedTeamId) return
    const content = contentRef.current
    if (!content) return
    // 等下一帧让 sections DOM 实际挂载（首次 open 时 sections 与 effect 同步执行）。
    const raf = requestAnimationFrame(() => {
      const target = content.querySelector<HTMLElement>(`[data-team-id="${CSS.escape(focusedTeamId)}"]`)
      if (target) {
        target.scrollIntoView({ block: 'start', behavior: 'auto' })
        userScrolledUp.current = true
        setShowJumpToBottom(true)
      }
      clearFocusedTeam(conversationId)
    })
    return () => cancelAnimationFrame(raf)
  }, [focusedTeamId, conversationId, clearFocusedTeam])

  if (!overview || overview.teams.length === 0) {
    return (
      <div className="flex h-full flex-col">
        <DrawerHeader
          title={t('team.process.title')}
          subtitle={t('team.process.emptySubtitle')}
          memberCount={0}
          onClose={onClose}
        />
        <div className="flex flex-1 items-center justify-center px-6 text-sm text-muted-foreground">
          {t('team.process.emptyBody')}
        </div>
      </div>
    )
  }

  return (
    <div className="relative flex h-full flex-col">
      <DrawerHeader
        title={t('team.process.title')}
        subtitle={t('team.process.sessionCount', { count: overview.teams.length })}
        memberCount={overview.teams.reduce((sum, t) => sum + t.members.filter((m) => !isLeadName(m.agentName)).length, 0)}
        onClose={onClose}
      />
      <div
        ref={scrollRef}
        className="flex-1 overflow-y-auto overscroll-contain px-4"
        onScroll={handleScroll}
        onWheel={markUserIntent}
        onTouchMove={markUserIntent}
        onKeyDown={markUserIntent}
      >
        <div ref={contentRef}>
          {overview.teams.map((team) => (
            <TeamSessionSection
              key={team.teamId}
              session={team}
              onDrill={(agentName) => {
                const agent = team.members.find((m) => m.agentName === agentName)
                if (agent) onDrill(agent.agentId)
              }}
            />
          ))}
        </div>
      </div>
      {showJumpToBottom ? (
        <button
          type="button"
          aria-label={t('team.process.jumpToBottom')}
          onClick={jumpToBottom}
          className="absolute bottom-4 left-1/2 z-20 flex h-9 w-9 -translate-x-1/2 items-center justify-center rounded-full border border-border bg-card text-muted-foreground shadow-[var(--shadow-card)] transition-colors hover:bg-muted hover:text-foreground"
        >
          <ArrowDown className="h-4 w-4" />
        </button>
      ) : null}
    </div>
  )
}

interface DrawerHeaderProps {
  title: string
  subtitle: string
  memberCount: number
  onClose: () => void
}

function DrawerHeader({ title, subtitle, memberCount, onClose }: DrawerHeaderProps) {
  const { t } = useTranslation()
  return (
    <div className="flex items-center gap-3 border-b border-border bg-muted/30 px-4 py-3">
      <h2 className="text-base font-medium text-foreground">{title}</h2>
      <span className="text-xs text-muted-foreground">{subtitle}</span>
      <span className="ml-auto shrink-0 text-xs text-muted-foreground">
        {t('team.progress.memberCount', { count: memberCount })}
      </span>
      <Button
        type="button"
        variant="ghost"
        size="icon"
        aria-label={t('team.process.close')}
        onClick={onClose}
        className="h-7 w-7"
      >
        <X className="h-4 w-4" />
      </Button>
    </div>
  )
}

interface TeamSessionSectionProps {
  session: TeamSession
  onDrill: (agentName: string) => void
}

function TeamSessionSection({ session, onDrill }: TeamSessionSectionProps) {
  const { t } = useTranslation()
  const teamVisual = useTeamVisualContext()
  const chatWidthMode = useSettingsStore((s) => s.chatWidthMode ?? 'full')
  const visibleMembers = session.members.filter((m) => !isLeadName(m.agentName))
  const isLive = session.deletedAt === null
  const title = teamVisual?.name ?? session.teamName ?? t('team.session.untitled')
  return (
    <section data-team-id={session.teamId} className="border-b border-border last:border-b-0">
      <div className="sticky top-0 z-10 -mx-4 border-b border-border bg-background/95 px-4 py-2 backdrop-blur">
        <div className="flex items-center justify-between gap-2 text-xs">
          <div className="flex min-w-0 items-center gap-2">
            <span className="truncate font-medium text-foreground">
              {title}
            </span>
            {isLive ? (
              <span className="inline-flex shrink-0 items-center gap-1 rounded-full bg-primary/10 px-1.5 py-0.5 text-[10px] font-medium text-primary">
                <span className="h-1.5 w-1.5 rounded-full bg-primary" />
                {t('team.session.live')}
              </span>
            ) : (
              <span
                className="inline-flex shrink-0 items-center gap-1 rounded-full bg-muted px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground"
                title={t('team.session.dismissedAt', { time: formatShortDateTime(session.deletedAt!) })}
              >
                <span className="h-1.5 w-1.5 rounded-full bg-muted-foreground/60" />
                {t('team.session.dismissed')}
              </span>
            )}
          </div>
          <span className="shrink-0 text-muted-foreground">
            {formatDuration(session.createdAt, session.deletedAt, t('team.session.live'))}
          </span>
        </div>
        {visibleMembers.length > 0 && (
          <div className="mt-2 flex flex-wrap items-center gap-2">
            {visibleMembers.map((member) => (
              <MemberButton
                key={member.agentId}
                agentName={member.agentName}
                hasTranscript={member.hasTranscript}
                onClick={() => onDrill(member.agentName)}
              />
            ))}
          </div>
        )}
      </div>

      <div className={chatWidthMode === 'full' ? 'w-full' : 'mx-auto w-full max-w-[736px]'}>
        <TeamChatEvents events={session.events} onDrillAgent={onDrill} />
      </div>
    </section>
  )
}

interface MemberButtonProps {
  agentName: string
  hasTranscript: boolean
  onClick: () => void
}

function MemberButton({ agentName, hasTranscript, onClick }: MemberButtonProps) {
  const { t } = useTranslation()
  const teamVisual = useTeamVisualContext()
  const displayName = getExpertDisplayName(teamVisual, agentName)
  return (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      disabled={!hasTranscript}
      onClick={onClick}
      className="h-7 gap-1.5 rounded-full px-2"
      title={hasTranscript
        ? t('team.process.viewMemberProcess', { name: displayName })
        : t('team.process.noMemberTranscript', { name: displayName })}
    >
      <AgentAvatar name={agentName} size="sm" />
      <span className="text-xs">{displayName}</span>
    </Button>
  )
}
