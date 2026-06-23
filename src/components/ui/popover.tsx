import * as React from 'react'
import * as PopoverPrimitive from '@radix-ui/react-popover'

import { cn } from '@/lib/utils'

const Popover = PopoverPrimitive.Root
const PopoverTrigger = PopoverPrimitive.Trigger

interface PopoverContentProps
  extends React.ComponentPropsWithoutRef<typeof PopoverPrimitive.Content> {
  portalled?: boolean
}

const PopoverContent = React.forwardRef<
  React.ElementRef<typeof PopoverPrimitive.Content>,
  PopoverContentProps
  // spec §7.14 — sideOffset=8 for breathing room (bigger than Tooltip's 4)
>(({ className, align = 'center', sideOffset = 8, portalled = true, ...props }, ref) => {
  const content = (
    <PopoverPrimitive.Content
      ref={ref}
      align={align}
      sideOffset={sideOffset}
      // spec §7.14 — Popover: rounded-md (12px, Containers tier), --shadow-lg
      // No fixed width — let content drive sizing.
      className={cn(
        'z-50 rounded-md border border-border bg-popover p-4 text-popover-foreground shadow-[var(--shadow-lg)] outline-none',
        className,
      )}
      {...props}
    />
  )

  return portalled ? <PopoverPrimitive.Portal>{content}</PopoverPrimitive.Portal> : content
})
PopoverContent.displayName = PopoverPrimitive.Content.displayName

export { Popover, PopoverTrigger, PopoverContent }
