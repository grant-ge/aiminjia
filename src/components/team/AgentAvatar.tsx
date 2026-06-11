import { cn } from '@/lib/utils'
import { ExpertAvatarView } from '@/features/expert-teams/ExpertAvatarView'
import { getExpertAvatarVisualForAgent } from '@/features/expert-teams/expertAvatar'
import { getExpertDisplayName } from '@/features/expert-teams/teams'
import { formatLeadDisplayName, getAgentIdentity, isLeadName } from './agentIdentity'
import { useTeamVisualContext } from './TeamVisualContext'

interface AgentAvatarProps {
  name: string
  size?: 'sm' | 'md' | 'lg'
  className?: string
}

const SIZE_CLASS = {
  sm: 'h-6 w-6 text-[10px]',
  md: 'h-8 w-8 text-xs',
  lg: 'h-10 w-10 text-sm',
} as const

const LEAD_AVATAR_VISUAL = { kind: 'image', url: '/expert-avatars/lead.svg' } as const

export function AgentAvatar({ name, size = 'md', className }: AgentAvatarProps) {
  const team = useTeamVisualContext()
  const isLead = isLeadName(name)
  const displayName = isLead ? formatLeadDisplayName(name) : getExpertDisplayName(team, name)
  const id = getAgentIdentity(displayName)
  const expertAvatarVisual = isLead ? LEAD_AVATAR_VISUAL : getExpertAvatarVisualForAgent(team, name)
  const hasImageAvatar =
    expertAvatarVisual?.kind === 'image' || expertAvatarVisual?.kind === 'atlas'
  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center justify-center font-semibold',
        SIZE_CLASS[size],
        hasImageAvatar
          ? 'overflow-hidden rounded-full border border-card bg-muted text-foreground'
          : cn('rounded-md', id.avatarClass),
        className,
      )}
      aria-label={displayName}
      title={displayName}
    >
      <ExpertAvatarView
        visual={expertAvatarVisual}
        fallback={id.initials}
        className={hasImageAvatar ? 'rounded-full' : undefined}
      />
    </span>
  )
}
