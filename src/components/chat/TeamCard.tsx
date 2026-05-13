/**
 * TeamCard — inline 团队卡片
 *
 * 在 main agent 触发 TeamCreate 之后，主消息流里插入这张卡片，作为打开右侧
 * 群聊抽屉的入口。
 *
 * 渲染条件：当前对话的 TeamView 含 team_name（即调过 TeamCreate）。
 * 显示信息：team_name / 描述 / 成员名册 / 任务进度。点击切换抽屉开关。
 */
import { Users } from 'lucide-react'
import { useUiStore } from '@/stores/uiStore'
import type { TeamRoster } from '@/types/team'

interface TeamCardProps {
  roster: TeamRoster
}

export function TeamCard({ roster }: TeamCardProps) {
  const teamDrawerOpen = useUiStore((s) => s.teamDrawerOpen)
  const toggleTeamDrawer = useUiStore((s) => s.toggleTeamDrawer)

  const teamName = roster.team_name ?? 'team'
  const memberCount = roster.members.length + 1 // +1 for team-lead
  const taskPct =
    roster.task_count_total > 0
      ? Math.round((roster.task_count_completed / roster.task_count_total) * 100)
      : 0

  return (
    <button
      type="button"
      onClick={toggleTeamDrawer}
      className={[
        'mx-auto my-3 block w-full max-w-[640px] rounded-xl border bg-card p-0 text-left transition-shadow',
        teamDrawerOpen
          ? 'border-primary shadow-[0_0_0_2px_var(--primary)]'
          : 'border-border hover:shadow-[var(--shadow-card)]',
      ].join(' ')}
      aria-label={teamDrawerOpen ? '收起群聊' : '打开群聊'}
    >
      {/* Header */}
      <div className="flex items-center gap-3 border-b border-border px-4 py-3">
        <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-primary text-primary-foreground">
          <Users className="h-4 w-4" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 text-sm font-semibold">
            项目群 {teamName}
            <span className="h-1.5 w-1.5 rounded-full bg-emerald-500" aria-hidden />
          </div>
          {roster.description ? (
            <div className="truncate text-xs text-muted-foreground">{roster.description}</div>
          ) : null}
        </div>
        <span className="text-xs font-medium text-primary">
          {teamDrawerOpen ? '收起 ←' : '展开 →'}
        </span>
      </div>

      {/* Body */}
      <div className="px-4 py-3">
        <div className="mb-2 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
          成员 ({memberCount})
        </div>
        <div className="flex flex-wrap gap-1.5">
          <MemberChip name="AI小家" tone="lead" />
          {roster.members.map((m) => (
            <MemberChip
              key={m.agent_id}
              name={m.name}
              tone={m.employee_id ? 'employee' : 'member'}
            />
          ))}
        </div>

        {roster.task_count_total > 0 ? (
          <div className="mt-3">
            <div className="mb-1.5 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
              任务进度
            </div>
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <div className="h-1 flex-1 overflow-hidden rounded-full bg-muted">
                <div
                  className="h-full bg-primary transition-[width] duration-300"
                  style={{ width: `${taskPct}%` }}
                />
              </div>
              <span>
                {roster.task_count_completed}/{roster.task_count_total} 完成
              </span>
            </div>
          </div>
        ) : null}
      </div>
    </button>
  )
}

function MemberChip({
  name,
  tone,
}: {
  name: string
  tone: 'lead' | 'member' | 'employee'
}) {
  const initial = name.charAt(0).toUpperCase()
  const avatarBg =
    tone === 'lead'
      ? 'bg-primary text-primary-foreground'
      : tone === 'employee'
        ? 'bg-amber-600 text-white'
        : 'bg-emerald-600 text-white'
  return (
    <span className="inline-flex items-center gap-1.5 rounded-full bg-muted py-0.5 pl-0.5 pr-2 text-xs">
      <span
        className={['flex h-4 w-4 items-center justify-center rounded-full text-[9px] font-semibold', avatarBg].join(' ')}
      >
        {initial}
      </span>
      <span className="truncate max-w-[120px]">{name}</span>
    </span>
  )
}
