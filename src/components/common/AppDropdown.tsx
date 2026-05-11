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
          // TODO: 自定义 shadow 不在 visual-standard 中，待 design tokens 补充统一阴影变量
          'min-w-40 rounded-lg border border-border bg-popover p-1.5 text-foreground shadow-[0_8px_24px_rgba(0,0,0,0.10),0_2px_6px_rgba(0,0,0,0.06)]',
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
              'gap-2.5 rounded-md px-2.5 py-2 text-base leading-5 text-foreground',
              '[&_svg]:h-3.5 [&_svg]:w-3.5 [&_svg]:shrink-0',
              item.disabled && 'cursor-not-allowed text-muted-foreground opacity-75',
              itemClassName,
              item.className,
            )}
          >
            {item.icon}
            <span>{item.label}</span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
