import * as React from 'react'
import * as TooltipPrimitive from '@radix-ui/react-tooltip'

import { cn } from '@/lib/utils'

// spec §7.13 — Tooltip is theme-neutral (dark on light, light on dark). Using
// bg-primary made tooltips invisible under gold tenant themes; bg-foreground
// is always the inverse of bg-background, which is what tooltips need.
const TooltipProvider: typeof TooltipPrimitive.Provider = ({ delayDuration = 600, ...props }) => (
  <TooltipPrimitive.Provider delayDuration={delayDuration} {...props} />
)
TooltipProvider.displayName = 'TooltipProvider'

const Tooltip = TooltipPrimitive.Root
const TooltipTrigger = TooltipPrimitive.Trigger

const TooltipContent = React.forwardRef<
  React.ElementRef<typeof TooltipPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof TooltipPrimitive.Content>
>(({ className, sideOffset = 4, ...props }, ref) => (
  <TooltipPrimitive.Portal>
    <TooltipPrimitive.Content
      ref={ref}
      sideOffset={sideOffset}
      className={cn(
        'z-50 overflow-hidden rounded-md bg-foreground px-3 py-1.5 text-xs text-background shadow-[var(--shadow-md)]',
        className,
      )}
      {...props}
    />
  </TooltipPrimitive.Portal>
))
TooltipContent.displayName = 'TooltipContent'

export { Tooltip, TooltipTrigger, TooltipContent, TooltipProvider }
