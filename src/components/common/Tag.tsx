import * as React from 'react'
import { Slot } from '@radix-ui/react-slot'
import { X } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

export type TagSize = 'xs' | 'sm' | 'md'
export type TagVariant = 'filled' | 'outlined' | 'solid'
export type TagColor =
  | 'default'
  | 'primary'
  | 'accent'
  | 'success'
  | 'warning'
  | 'destructive'

const sizeClasses: Record<TagSize, string> = {
  xs: 'h-[18px] px-1.5 text-[10px]',
  sm: 'h-5 px-1.5 text-xs',
  md: 'h-7 px-2 text-xs',
}

const iconSizeClasses: Record<TagSize, string> = {
  xs: 'h-3 w-3',
  sm: 'h-3.5 w-3.5',
  md: 'h-3.5 w-3.5',
}

const closeSizeClasses: Record<TagSize, string> = {
  xs: 'h-3.5 w-3.5',
  sm: 'h-4 w-4',
  md: 'h-4.5 w-4.5',
}

const variantClasses: Record<TagVariant, Record<TagColor, string>> = {
  filled: {
    default: 'border-transparent bg-muted text-muted-foreground',
    primary: 'border-transparent bg-primary/10 text-primary',
    accent: 'border-transparent bg-accent text-accent-foreground',
    success: 'border-transparent bg-success/10 text-success',
    warning: 'border-transparent bg-warning/15 text-warning',
    destructive: 'border-transparent bg-destructive/10 text-destructive',
  },
  outlined: {
    default: 'border-border bg-transparent text-muted-foreground',
    primary: 'border-primary/30 bg-transparent text-primary',
    accent: 'border-accent bg-transparent text-foreground',
    success: 'border-success/40 bg-transparent text-success',
    warning: 'border-warning/40 bg-transparent text-warning',
    destructive: 'border-destructive/40 bg-transparent text-destructive',
  },
  solid: {
    default: 'border-muted bg-muted text-foreground',
    primary: 'border-primary bg-primary text-primary-foreground',
    accent: 'border-accent bg-accent text-accent-foreground',
    success: 'border-success bg-success text-success-foreground',
    warning: 'border-warning bg-warning text-warning-foreground',
    destructive: 'border-destructive bg-destructive text-destructive-foreground',
  },
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

export interface TagProps extends React.HTMLAttributes<HTMLElement> {
  asButton?: boolean
  asChild?: boolean
  closeLabel?: string
  color?: TagColor
  icon?: React.ReactNode
  onClose?: (event: React.MouseEvent<HTMLButtonElement>) => void
  size?: TagSize
  variant?: TagVariant
}

/**
 * 通用标签组件，用于展示属性、分类、状态和轻量 token。
 *
 * 不用于互斥选择、开关或 tab；这些场景继续使用 SegmentedControl。
 * 需要主操作或次操作时使用 Button。
 */
export function Tag({
  asButton = false,
  asChild = false,
  children,
  className,
  closeLabel = '移除',
  color = 'default',
  icon,
  onClose,
  onClick,
  onKeyDown,
  size = 'sm',
  variant = 'filled',
  ...props
}: TagProps) {
  const clickable = asButton || Boolean(onClick)
  const rootClassName = cn(
    'inline-flex max-w-full items-center gap-1 rounded border font-medium leading-none transition-colors',
    sizeClasses[size],
    variantClasses[variant][color],
    clickable && 'cursor-pointer hover:opacity-85 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
    props['aria-disabled'] && 'pointer-events-none opacity-50',
    className,
  )

  if (asChild) {
    return (
      <Slot
        className={rootClassName}
        onClick={onClick}
        onKeyDown={onKeyDown}
        {...props}
      >
        {children}
      </Slot>
    )
  }

  return (
    <span
      className={rootClassName}
      onClick={onClick}
      onKeyDown={(event) => {
        onKeyDown?.(event)
        if (!clickable || event.defaultPrevented) return
        if (event.key !== 'Enter' && event.key !== ' ') return
        event.preventDefault()
        event.currentTarget.click()
      }}
      role={!asChild && clickable ? 'button' : props.role}
      tabIndex={!asChild && clickable ? (props.tabIndex ?? 0) : props.tabIndex}
      {...props}
    >
      {icon ? renderIcon(icon, iconSizeClasses[size]) : null}
      {children}
      {onClose ? (
        <Button unstyled
          type="button"
          aria-label={closeLabel}
          onClick={(event) => {
            event.preventDefault()
            event.stopPropagation()
            onClose(event)
          }}
          className={cn(
            '-mr-0.5 inline-flex shrink-0 items-center justify-center rounded text-current opacity-70 transition-colors hover:bg-foreground/10 hover:opacity-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
            closeSizeClasses[size],
          )}
        >
          <X className={cn('h-3 w-3', size === 'xs' && 'h-2.5 w-2.5')} />
        </Button>
      ) : null}
    </span>
  )
}
