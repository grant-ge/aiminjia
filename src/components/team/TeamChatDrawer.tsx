import { useMemo } from 'react'
import { X } from 'lucide-react'

import { Button } from '@/components/ui/button'
import type { TeamOverview, TeamSession } from '@/types/team'
import { useConversationTeamState, useTeamStore } from '@/stores/teamStore'

import { AgentAvatar } from './AgentAvatar'
import { TeamChatEvents } from './TeamChatEvents'
import { TeammateDetailPanel } from './TeammateDetailPanel'
import { formatDuration } from './formatters'
import { isLeadName } from './agentIdentity'

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
      className="relative flex h-full w-[500px] shrink-0 flex-col border-l border-border bg-background"
    >
      <Button
        type="button"
        variant="ghost"
        size="icon"
        aria-label="关闭团队面板"
        onClick={() => closeDrawer(conversationId, true)}
        className="absolute right-2 top-2 z-10 h-7 w-7"
      >
        <X className="h-4 w-4" />
      </Button>
      {drillAgent ? (
        <TeammateDetailPanel
          conversationId={conversationId}
          agentId={drillAgent.agentId}
          agentName={drillAgent.agentName}
          onBack={() => setDrillAgent(conversationId, null)}
        />
      ) : (
        <DrawerOverview overview={overview} onDrill={(agentId) => setDrillAgent(conversationId, agentId)} />
      )}
    </aside>
  )
}

interface DrawerOverviewProps {
  overview: TeamOverview | null
  onDrill: (agentId: string) => void
}

function DrawerOverview({ overview, onDrill }: DrawerOverviewProps) {
  if (!overview || overview.teams.length === 0) {
    return (
      <div className="flex h-full flex-col">
        <DrawerHeader title="团队过程" subtitle="没有团队记录" memberCount={0} />
        <div className="flex flex-1 items-center justify-center px-6 text-sm text-muted-foreground">
          这个会话还没有创建团队。
        </div>
      </div>
    )
  }

  return (
    <div className="flex h-full flex-col">
      <DrawerHeader
        title="团队过程"
        subtitle={`${overview.teams.length} 个团队会话`}
        memberCount={overview.teams.reduce((sum, t) => sum + t.members.filter((m) => !isLeadName(m.agentName)).length, 0)}
      />
      <div className="flex-1 overflow-y-auto px-4">
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
  )
}

interface DrawerHeaderProps {
  title: string
  subtitle: string
  memberCount: number
}

function DrawerHeader({ title, subtitle, memberCount }: DrawerHeaderProps) {
  return (
    <div className="border-b border-border bg-muted/30 px-4 py-3 pr-12">
      <div className="flex items-baseline justify-between gap-2">
        <h2 className="text-base font-medium text-foreground">{title}</h2>
        <span className="text-xs text-muted-foreground">{memberCount} 位成员</span>
      </div>
      <p className="mt-0.5 text-xs text-muted-foreground">{subtitle}</p>
    </div>
  )
}

interface TeamSessionSectionProps {
  session: TeamSession
  onDrill: (agentName: string) => void
}

function TeamSessionSection({ session, onDrill }: TeamSessionSectionProps) {
  const visibleMembers = session.members.filter((m) => !isLeadName(m.agentName))
  return (
    <section className="border-b border-border last:border-b-0">
      <div className="sticky top-0 z-10 -mx-4 border-b border-border bg-background/95 px-4 py-2 backdrop-blur">
        <div className="flex items-center justify-between gap-2 text-xs">
          <div className="flex min-w-0 items-center gap-2">
            <span className="truncate font-medium text-foreground">
              {session.teamName ?? '团队对话'}
            </span>
            {session.deletedAt === null && (
              <span className="inline-flex shrink-0 items-center gap-1 rounded-full bg-emerald-500/15 px-1.5 py-0.5 text-[10px] font-medium text-emerald-700 dark:text-emerald-300">
                <span className="h-1.5 w-1.5 rounded-full bg-emerald-500" />
                进行中
              </span>
            )}
          </div>
          <span className="shrink-0 text-muted-foreground">
            {formatDuration(session.createdAt, session.deletedAt)}
          </span>
        </div>
        {visibleMembers.length > 0 && (
          <div className="mt-2 flex flex-wrap items-center gap-2">
            {visibleMembers.map((member) => (
              <Button
                key={member.agentId}
                type="button"
                variant="ghost"
                size="sm"
                disabled={!member.hasTranscript}
                onClick={() => onDrill(member.agentName)}
                className="h-7 gap-1.5 rounded-full px-2"
                title={member.hasTranscript ? `查看 ${member.agentName} 的过程` : `${member.agentName}（无可下钻记录）`}
              >
                <AgentAvatar name={member.agentName} size="sm" />
                <span className="text-xs">{member.agentName}</span>
              </Button>
            ))}
          </div>
        )}
      </div>

      <TeamChatEvents events={session.events} onDrillAgent={onDrill} />
    </section>
  )
}
