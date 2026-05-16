// code/src/components/chat-scene/ExpertTeamBanner.tsx
import { X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { clearExpertTeam } from '@/features/expert-teams/expertTeamRegistry'
import { getExpertTeam, type ExpertTeamId } from '@/features/expert-teams/teams'

interface ExpertTeamBannerProps {
  conversationId: string
  teamId: ExpertTeamId
}

export function ExpertTeamBanner({ conversationId, teamId }: ExpertTeamBannerProps) {
  const team = getExpertTeam(teamId)
  if (!team) return null

  const initials = team.experts.slice(0, 4).map((e) => e.name.slice(0, 1))

  return (
    <div className="flex items-center gap-3 border-b border-border bg-primary/8 px-4 py-2 text-sm text-foreground">
      <span className="text-base leading-none" aria-hidden>
        {team.emoji}
      </span>
      <span className="font-medium">{team.name}</span>
      {initials.length > 0 && (
        <div className="flex items-center -space-x-1">
          {initials.map((char, idx) => (
            <span
              key={`${char}-${idx}`}
              className="flex h-5 w-5 items-center justify-center rounded-full border border-border bg-card text-[10px] text-muted-foreground"
            >
              {char}
            </span>
          ))}
        </div>
      )}
      <span className="ml-auto" />
      <Button
        variant="ghost"
        size="icon"
        className="h-6 w-6"
        aria-label="关闭专家团"
        onClick={() => clearExpertTeam(conversationId)}
      >
        <X className="h-3.5 w-3.5" />
      </Button>
    </div>
  )
}
