import type { TeamSession } from '@/types/team'
import { cn } from '@/lib/utils'
import { AgentAvatar } from './AgentAvatar'
import { formatDuration } from './formatters'
import { isLeadName } from './agentIdentity'

interface TeamProgressBlockProps {
  session: TeamSession
  onOpen: () => void
}

/**
 * In-conversation card representing a single team session.
 *
 * Visual goal: a quiet, restrained "anchor" block that announces a team
 * existed here, lists its members, and invites the user into the drawer.
 * Per the design contract, the block is deliberately static (no live
 * counters or summaries) — the LLM's surrounding text is responsible for
 * narrating progress.
 */
export function TeamProgressBlock({ session, onOpen }: TeamProgressBlockProps) {
  const title = session.teamName ?? '团队对话'
  // Don't show team-lead as an avatar — it's the conversation itself.
  const visibleMembers = session.members.filter((m) => !isLeadName(m.agentName))
  const memberCount = visibleMembers.length
  const messageCount = session.events.filter(
    (e) => e.kind === 'send_message' || e.kind === 'peer_message',
  ).length

  return (
    <button
      type="button"
      onClick={onOpen}
      className={cn(
        'group flex w-full items-center gap-3 rounded-lg border border-border bg-sidebar px-4 py-3 text-left transition-colors',
        'hover:border-primary/40 hover:bg-sidebar-accent',
      )}
    >
      <div className="flex -space-x-2">
        {visibleMembers.slice(0, 5).map((member) => (
          <AgentAvatar
            key={member.agentId}
            name={member.agentName}
            size="md"
          />
        ))}
        {memberCount > 5 && (
          <span className="inline-flex h-8 w-8 items-center justify-center rounded-full bg-muted text-xs text-muted-foreground">
            +{memberCount - 5}
          </span>
        )}
      </div>

      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex items-center gap-2 text-sm font-medium text-foreground">
          <span className="truncate">{title}</span>
          {session.deletedAt === null && (
            <span className="inline-flex shrink-0 items-center gap-1 rounded-full bg-emerald-500/15 px-1.5 py-0.5 text-[10px] font-medium text-emerald-700 dark:text-emerald-300">
              <span className="h-1.5 w-1.5 rounded-full bg-emerald-500" />
              进行中
            </span>
          )}
        </div>
        <div className="truncate text-xs text-muted-foreground">
          {memberCount} 位成员 · {messageCount} 条消息 · {formatDuration(session.createdAt, session.deletedAt)}
        </div>
      </div>

      <span className="shrink-0 text-xs text-muted-foreground transition-colors group-hover:text-primary">
        查看过程 →
      </span>
    </button>
  )
}
