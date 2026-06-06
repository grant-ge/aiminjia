import type { CSSProperties, ReactNode } from 'react'

import { cn } from '@/lib/utils'
import type { ExpertAvatarVisual } from './expertAvatar'

interface ExpertAvatarViewProps {
  visual: ExpertAvatarVisual | null
  fallback: ReactNode
  className?: string
}

function atlasStyle(visual: Extract<ExpertAvatarVisual, { kind: 'atlas' }>): CSSProperties {
  const xDenominator = Math.max(visual.atlasWidth - visual.w, 1)
  const yDenominator = Math.max(visual.atlasHeight - visual.h, 1)
  return {
    backgroundImage: `url(${visual.url})`,
    backgroundSize: `${(visual.atlasWidth / visual.w) * 100}% ${(visual.atlasHeight / visual.h) * 100}%`,
    backgroundPosition: `${(visual.x / xDenominator) * 100}% ${(visual.y / yDenominator) * 100}%`,
  }
}

export function ExpertAvatarView({ visual, fallback, className }: ExpertAvatarViewProps) {
  if (visual?.kind === 'image') {
    return <img src={visual.url} alt="" className={cn('h-full w-full object-cover', className)} />
  }
  if (visual?.kind === 'atlas') {
    return (
      <span
        aria-hidden
        className={cn('block h-full w-full bg-cover bg-no-repeat', className)}
        style={atlasStyle(visual)}
      />
    )
  }
  if (visual?.kind === 'text') {
    return <span aria-hidden className={className}>{visual.text}</span>
  }
  return <span aria-hidden className={className}>{fallback}</span>
}
