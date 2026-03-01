/**
 * Button — primary, secondary, ghost variants.
 * Based on visual-standard.md §7.1.
 */
import { useState, type ButtonHTMLAttributes, type ReactNode } from 'react'

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: 'primary' | 'secondary' | 'ghost'
  children: ReactNode
}

export function Button({
  variant = 'secondary',
  children,
  className = '',
  style,
  ...props
}: ButtonProps) {
  const [hovered, setHovered] = useState(false)

  const baseClasses =
    'inline-flex items-center gap-1.5 rounded-sm px-[18px] py-2 text-base font-medium cursor-pointer transition-all duration-150'

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
      className={`${baseClasses} border ${className}`}
      style={{ ...variantStyles[variant], ...style }}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      {...props}
    >
      {children}
    </button>
  )
}
