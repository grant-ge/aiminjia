import * as React from 'react'

import { cn } from '@/lib/utils'

type SpinnerSize = 'xs' | 'sm' | 'md' | 'lg'

const sizeClasses: Record<SpinnerSize, string> = {
  xs: 'h-3 w-3 border',
  sm: 'h-3.5 w-3.5 border',
  md: 'h-4 w-4 border',
  lg: 'h-8 w-8 border',
}

export interface SpinnerProps extends React.HTMLAttributes<HTMLSpanElement> {
  size?: SpinnerSize
}

export function Spinner({ className, size = 'md', ...props }: SpinnerProps) {
  return (
    <span
      {...props}
      className={cn(
        'inline-block box-border shrink-0 origin-center rounded-full border-t-current border-b-current border-l-current border-r-transparent animate-spin [will-change:transform] motion-reduce:animate-none',
        sizeClasses[size],
        className,
      )}
    />
  )
}
