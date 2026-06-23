import type { ButtonHTMLAttributes } from 'react'

import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'

interface SwitchProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'checked' | 'onChange' | 'role'> {
  checked: boolean
  onCheckedChange?: (checked: boolean) => void
  size?: 'sm' | 'md'
}

export function Switch({
  checked,
  onCheckedChange,
  disabled,
  className,
  size = 'md',
  ...props
}: SwitchProps) {
  const handleClick = () => {
    if (disabled) return
    onCheckedChange?.(!checked)
  }

  return (
    <Button unstyled
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={handleClick}
      className={cn(
        'relative h-6 w-11 shrink-0 rounded-md transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2',
        size === 'sm' && 'h-5 w-9',
        checked ? 'bg-primary' : 'bg-muted-foreground/30',
        disabled && 'cursor-not-allowed bg-muted-foreground/20',
        className,
      )}
      {...props}
    >
      <span
        className={cn(
          // Switch thumb 固定白色：在主题色 / 灰底轨道上保证对比度，跨 light/dark 都成立
          'absolute left-0.5 top-0.5 h-5 w-5 rounded-md bg-white shadow transition-transform',
          size === 'sm' && 'h-4 w-4',
          checked ? (size === 'sm' ? 'translate-x-4' : 'translate-x-5') : 'translate-x-0',
          disabled && 'bg-card',
        )}
      />
    </Button>
  )
}
