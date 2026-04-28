import type { SelectHTMLAttributes } from 'react'
import { ChevronDown } from 'lucide-react'

import { cn } from '@/lib/utils'

export interface SelectOption {
  value: string
  label: string
}

interface SelectProps extends Omit<SelectHTMLAttributes<HTMLSelectElement>, 'onChange'> {
  options: SelectOption[]
  onValueChange?: (value: string) => void
}

export function Select({
  options,
  onValueChange,
  disabled,
  className,
  ...props
}: SelectProps) {
  return (
    <span className={cn('relative inline-flex min-w-36 shrink-0', disabled && 'opacity-60')}>
      <select
        disabled={disabled}
        onChange={(event) => onValueChange?.(event.target.value)}
        className={cn(
          'h-10 w-full appearance-none rounded-[12px] border border-border bg-muted py-0 pl-4 pr-9 text-sm font-semibold text-foreground outline-none transition-colors',
          'hover:bg-sidebar-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2',
          disabled && 'cursor-not-allowed hover:bg-muted',
          className,
        )}
        {...props}
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
      <ChevronDown
        className="pointer-events-none absolute right-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground"
        aria-hidden="true"
      />
    </span>
  )
}
