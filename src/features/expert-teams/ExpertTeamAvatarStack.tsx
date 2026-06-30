import { CircleQuestionMark } from 'lucide-react'

import { ExpertAvatarView } from './ExpertAvatarView'
import { getExpertAvatarVisual, getRoundtablePlaceholderAvatarUrl } from './expertAvatar'
import type { ExpertTeam } from './teams'
import { cn } from '@/lib/utils'

interface ExpertTeamAvatarStackProps {
  team: ExpertTeam
  size?: 'xs' | 'sm' | 'lg'
}

const SIZE_CLASS = {
  xs: {
    root: 'h-8 w-10',
    avatar: 'h-[22px] w-[22px]',
    questionBadge: 'h-2.5 w-2.5',
    questionIcon: 'h-1.5 w-1.5',
    text: 'text-[10px]',
  },
  sm: {
    root: 'h-10 w-12',
    avatar: 'h-7 w-7',
    questionBadge: 'h-3 w-3',
    questionIcon: 'h-2 w-2',
    text: 'text-xs',
  },
  lg: {
    root: 'h-14 w-16',
    avatar: 'h-10 w-10',
    questionBadge: 'h-4 w-4',
    questionIcon: 'h-2.5 w-2.5',
    text: 'text-sm',
  },
}

const POSITION_CLASS = [
  'left-0 top-0 z-10',
  'right-0 top-0 z-20',
  'left-1/2 top-[38%] z-30 -translate-x-1/2',
]

export function ExpertTeamAvatarStack({ team, size = 'sm' }: ExpertTeamAvatarStackProps) {
  const classes = SIZE_CLASS[size]
  const experts = team.experts.slice(0, 3)

  if (experts.length === 0) {
    return (
      <span
        className={cn('relative block shrink-0', classes.root)}
        data-aijia-expert-team-avatar-stack
        aria-hidden
      >
        {[0, 1, 2].map((index) => (
          <span
            key={index}
            className={cn(
              'absolute flex items-center justify-center overflow-hidden rounded-full border-2 border-card bg-muted shadow-[0_1px_2px_rgba(0,0,0,0.08)]',
              classes.avatar,
              POSITION_CLASS[index],
            )}
          >
            <ExpertAvatarView
              visual={{ kind: 'image', url: getRoundtablePlaceholderAvatarUrl(index) }}
              fallback=""
              className={cn('leading-none grayscale', classes.text)}
            />
            <span
              className={cn(
                'absolute inset-0 m-auto flex items-center justify-center rounded-full border border-card bg-[rgba(var(--background-rgb),0.90)] text-[rgba(var(--foreground-rgb),0.80)] shadow-[0_1px_2px_rgba(0,0,0,0.08)]',
                classes.questionBadge,
              )}
            >
              <CircleQuestionMark className={classes.questionIcon} strokeWidth={2.4} />
            </span>
          </span>
        ))}
      </span>
    )
  }

  return (
    <span
      className={cn('relative block shrink-0', classes.root)}
      data-aijia-expert-team-avatar-stack
      aria-hidden
    >
      {experts.map((expert, index) => {
        const avatarVisual = getExpertAvatarVisual(team.id, expert)
        return (
          <span
            key={`${expert.name}-${index}`}
            className={cn(
              'absolute flex items-center justify-center overflow-hidden rounded-full border-2 border-card bg-muted shadow-[0_1px_2px_rgba(0,0,0,0.08)]',
              classes.avatar,
              POSITION_CLASS[index],
            )}
          >
            <ExpertAvatarView
              visual={avatarVisual}
              fallback={expert.emoji || Array.from(expert.name)[0]}
              className={cn('leading-none', classes.text)}
            />
          </span>
        )
      })}
    </span>
  )
}
