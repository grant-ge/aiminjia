import * as React from 'react'
import { Slot } from '@radix-ui/react-slot'
import { cva, type VariantProps } from 'class-variance-authority'

import { cn } from '@/lib/utils'

const buttonVariants = cva(
  // spec §7.1 — controls use rounded-md (8px), text-sm
  'inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-colors disabled:pointer-events-none disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
  {
    variants: {
      variant: {
        default:
          'bg-primary text-primary-foreground hover:brightness-110 active:brightness-95',
        secondary:
          'bg-secondary text-secondary-foreground hover:bg-muted active:bg-muted/80',
        ghost:
          'text-foreground hover:bg-accent hover:text-accent-foreground active:bg-accent/80',
        destructive:
          'bg-destructive text-destructive-foreground hover:brightness-110 active:brightness-95',
        outline: 'border border-input bg-background hover:bg-accent hover:text-accent-foreground',
        link: 'text-primary underline-offset-4 hover:underline',
      },
      size: {
        // spec §7.1
        default: 'h-9 px-3.5 py-2',
        sm: 'h-7 px-2.5 text-xs',
        // lg — page-level CTA (login submit, paywall confirm). Pill radius
        // intentional: large primary buttons read as "go" affordances.
        lg: 'h-11 px-8 text-md font-semibold rounded-full',
        icon: 'h-9 w-9',
      },
    },
    defaultVariants: {
      variant: 'default',
      size: 'default',
    },
  },
)

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, ...props }, ref) => {
    const Comp = asChild ? Slot : 'button'
    return <Comp className={cn(buttonVariants({ variant, size, className }))} ref={ref} {...props} />
  },
)
Button.displayName = 'Button'

export { Button }
