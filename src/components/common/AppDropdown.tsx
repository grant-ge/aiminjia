import type { ComponentPropsWithoutRef, ReactElement, ReactNode } from 'react'
import { cloneElement, isValidElement } from 'react'

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { cn } from '@/lib/utils'

export interface AppDropdownItem {
  id: string
  label: ReactNode
  icon?: ReactNode
  disabled?: boolean
  className?: string
  onSelect?: (event: Event) => void
  /** Optional data-* attributes to passthrough to the rendered item (e.g. e2e selectors). */
  dataAttrs?: Record<string, string>
}

interface AppDropdownProps {
  trigger: ReactElement
  ariaLabel: string
  items: AppDropdownItem[]
  align?: ComponentPropsWithoutRef<typeof DropdownMenuContent>['align']
  side?: ComponentPropsWithoutRef<typeof DropdownMenuContent>['side']
  sideOffset?: ComponentPropsWithoutRef<typeof DropdownMenuContent>['sideOffset']
  open?: boolean
  onOpenChange?: (open: boolean) => void
  contentClassName?: string
  itemClassName?: string
}

function withTriggerLabel(trigger: ReactElement, ariaLabel: string): ReactElement {
  if (!isValidElement(trigger)) return trigger
  return cloneElement(trigger, { 'aria-label': ariaLabel } as Partial<unknown>)
}

export function AppDropdown({
  trigger,
  ariaLabel,
  items,
  align = 'end',
  side,
  sideOffset,
  open,
  onOpenChange,
  contentClassName,
  itemClassName,
}: AppDropdownProps) {
  return (
    <DropdownMenu open={open} onOpenChange={onOpenChange}>
      <DropdownMenuTrigger asChild>{withTriggerLabel(trigger, ariaLabel)}</DropdownMenuTrigger>
      <DropdownMenuContent
        align={align}
        side={side}
        sideOffset={sideOffset}
        className={cn(
          // spec §5 — uses --shadow-popover (already covered by DropdownMenuContent default,
          // but kept explicit here for the wider min-width variant)
          'min-w-40 rounded-md border border-border bg-popover p-1.5 text-foreground shadow-[var(--shadow-popover)]',
          '[&_[data-highlighted]]:bg-accent [&_[data-highlighted]]:text-accent-foreground',
          contentClassName,
        )}
      >
        {items.map((item) => (
          <DropdownMenuItem
            key={item.id}
            disabled={item.disabled}
            onSelect={item.onSelect}
            className={cn(
              'gap-2.5 rounded-md px-2.5 py-2 text-sm leading-5 text-foreground',
              '[&_svg]:h-3.5 [&_svg]:w-3.5 [&_svg]:shrink-0',
              item.disabled && 'cursor-not-allowed text-muted-foreground opacity-75',
              itemClassName,
              item.className,
            )}
            {...(item.dataAttrs ?? {})}
          >
            {item.icon}
            <span>{item.label}</span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
