import { cn } from '@/lib/utils'
import { getExpertAvatarUrlForAgent } from '@/features/expert-teams/expertAvatar'
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
  const id = getAgentIdentity(name)
  const team = useTeamVisualContext()
  const expertAvatarUrl = getExpertAvatarUrlForAgent(team, name)
  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center justify-center rounded-full font-semibold tracking-tight',
        SIZE_CLASS[size],
        id.avatarClass,
        className,
      )}
      aria-label={name}
      title={name}
    >
      {expertAvatarUrl ? (
        <img src={expertAvatarUrl} alt="" className="h-full w-full rounded-full object-cover" />
      ) : (
        id.initials
      )}
    </span>
  )
}
