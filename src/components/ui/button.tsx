import * as React from 'react'
import { Slot } from '@radix-ui/react-slot'
import { Loader2 } from 'lucide-react'

import { cn } from '@/lib/utils'

type ButtonSize = 'sm' | 'md' | 'lg' | 'icon' | 'default'
type ButtonVariant = 'default' | 'primary' | 'secondary' | 'ghost' | 'outline' | 'destructive' | 'link'

const sizeClasses: Record<'sm' | 'md' | 'lg', string> = {
  sm: 'h-6 px-[7px] text-xs',
  md: 'h-8 px-[15px] text-sm',
  lg: 'h-10 px-[15px] text-sm',
}

const iconSizeClasses: Record<'sm' | 'md' | 'lg', string> = {
  sm: 'h-6 w-6 p-0',
  md: 'h-8 w-8 p-0',
  lg: 'h-10 w-10 p-0',
}

const iconGraphicSizeClasses: Record<'sm' | 'md' | 'lg', string> = {
  sm: 'h-3.5 w-3.5',
  md: 'h-4 w-4',
  lg: 'h-4 w-4',
}

const variantClasses: Record<Exclude<ButtonVariant, 'link'>, string> = {
  default: 'border border-primary bg-primary text-primary-foreground',
  primary: 'border border-primary bg-primary text-primary-foreground',
  secondary: 'border border-transparent bg-secondary text-secondary-foreground',
  ghost: 'border border-transparent bg-transparent text-muted-foreground',
  outline: 'border border-input bg-card text-foreground',
  destructive: 'border border-destructive bg-destructive text-destructive-foreground',
}

function normalizeSize(size: ButtonSize | null | undefined): 'sm' | 'md' | 'lg' {
  if (size === 'sm' || size === 'lg') return size
  return 'md'
}

type IconElementProps = {
  className?: string
  'aria-hidden'?: boolean | 'true' | 'false'
}

function renderIcon(icon: React.ReactNode, className: string): React.ReactNode {
  if (!React.isValidElement<IconElementProps>(icon)) return icon
  return React.cloneElement(icon, {
    className: cn(className, 'shrink-0', icon.props.className),
    'aria-hidden': icon.props['aria-hidden'] ?? true,
  })
}

function normalizeVariantClassKey(
  variant: ButtonVariant,
  isDanger: boolean,
): Exclude<ButtonVariant, 'link'> {
  if (isDanger) return 'destructive'
  if (variant === 'link') return 'default'
  return variant
}

export interface ButtonProps
  extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'disabled'> {
  asChild?: boolean
  block?: boolean
  danger?: boolean
  disabled?: boolean
  icon?: React.ReactNode
  link?: boolean
  loading?: boolean
  size?: ButtonSize
  unstyled?: boolean
  variant?: ButtonVariant
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  (
    {
      asChild = false,
      block = false,
      children,
      className,
      danger = false,
      disabled = false,
      icon,
      link = false,
      loading = false,
      size = 'md',
      unstyled = false,
      variant = 'default',
      ...props
    },
    ref,
  ) => {
    const Comp = asChild ? Slot : 'button'
    const normalizedSize = normalizeSize(size)
    const isLink = link || variant === 'link'
    const isDanger = danger || variant === 'destructive'
    const isDisabled = disabled || loading
    const hasChildren = React.Children.count(children) > 0
    const contentIcon = loading ? <Loader2 className="animate-spin" /> : icon
    const iconOnly = !isLink && (size === 'icon' || (!hasChildren && Boolean(contentIcon)))
    const variantClassKey = normalizeVariantClassKey(variant, isDanger)

    if (unstyled) {
      return (
        <Comp
          ref={ref}
          aria-busy={loading ? true : props['aria-busy']}
          className={className}
          disabled={isDisabled}
          data-loading={loading ? 'true' : undefined}
          {...props}
        >
          {contentIcon ? renderIcon(contentIcon, iconGraphicSizeClasses[normalizedSize]) : null}
          {children}
        </Comp>
      )
    }

    return (
      <Comp
        ref={ref}
        aria-busy={loading ? true : props['aria-busy']}
        className={cn(
          block ? 'flex w-full' : 'inline-flex',
          'items-center justify-center whitespace-nowrap rounded-md font-medium',
          isLink
            ? 'transition-colors duration-150 ease-out'
            : 'transition-[opacity,transform] duration-150 ease-out hover:opacity-90',
          'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-0',
          'disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-45',
          isLink
            ? 'h-auto border-transparent bg-transparent p-0 text-primary active:scale-100'
            : [
                'active:scale-[0.98]',
                variantClasses[variantClassKey],
                iconOnly ? iconSizeClasses[normalizedSize] : sizeClasses[normalizedSize],
                hasChildren && contentIcon ? 'gap-1.5' : null,
              ],
          className,
        )}
        disabled={isDisabled}
        data-loading={loading ? 'true' : undefined}
        {...props}
      >
        {contentIcon ? renderIcon(contentIcon, iconGraphicSizeClasses[normalizedSize]) : null}
        {children}
      </Comp>
    )
  },
)
Button.displayName = 'Button'

export { Button }
