import { cn } from '@/lib/utils'
import { ExpertAvatarView } from '@/features/expert-teams/ExpertAvatarView'
import { getExpertAvatarVisualForAgent } from '@/features/expert-teams/expertAvatar'
import { getExpertDisplayName } from '@/features/expert-teams/teams'
import { getAgentIdentity } from './agentIdentity'
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

export function AgentAvatar({ name, size = 'md', className }: AgentAvatarProps) {
  const team = useTeamVisualContext()
  const displayName = getExpertDisplayName(team, name)
  const id = getAgentIdentity(displayName)
  const expertAvatarVisual = getExpertAvatarVisualForAgent(team, name)
  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center justify-center rounded-full font-semibold tracking-tight',
        SIZE_CLASS[size],
        id.avatarClass,
        className,
      )}
      aria-label={displayName}
      title={displayName}
    >
      <ExpertAvatarView
        visual={expertAvatarVisual}
        fallback={id.initials}
        className={expertAvatarVisual?.kind === 'text' ? undefined : 'rounded-full'}
      />
    </span>
  )
}
