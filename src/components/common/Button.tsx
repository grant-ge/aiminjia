/**
 * Button — primary, secondary, ghost variants with consistent sizing.
 * Based on visual-standard.md §7.1.
 *
 * Sizes:
 *   md (default) — h-9 (36px), matches input height for inline pairing
 *   sm           — h-7 (28px), compact for inline actions (file cards, etc.)
 */
import { useState, type ButtonHTMLAttributes, type ReactNode } from 'react'

type ButtonSize = 'sm' | 'md'

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: 'primary' | 'secondary' | 'ghost'
  size?: ButtonSize
  children: ReactNode
}

const SIZE_CLASSES: Record<ButtonSize, string> = {
  sm: 'h-7 px-2.5 text-xs',
  md: 'h-9 px-3.5 text-sm',
}

export function Button({
  variant = 'secondary',
  size = 'md',
  children,
  className = '',
  style,
  ...props
}: ButtonProps) {
  const [hovered, setHovered] = useState(false)

  const baseClasses =
    'inline-flex items-center justify-center gap-1.5 rounded-md font-medium cursor-pointer transition-all duration-150 whitespace-nowrap'

  const variantStyles: Record<string, React.CSSProperties> = {
    primary: {
      background: hovered ? 'var(--color-primary-hover)' : 'var(--color-primary)',
      borderColor: hovered ? 'var(--color-primary-hover)' : 'var(--color-primary)',
      color: 'var(--color-text-on-primary)',
    },
    secondary: {
      background: 'var(--color-bg-card)',
      borderColor: 'var(--color-border)',
      color: 'var(--color-text-primary)',
    },
    ghost: {
      background: 'transparent',
      borderColor: 'transparent',
      color: 'var(--color-text-muted)',
    },
  }

  return (
    <button
      className={`${baseClasses} ${SIZE_CLASSES[size]} border ${className}`}
      style={{ ...variantStyles[variant], ...style }}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      {...props}
    >
      {children}
    </button>
  )
}
