/**
 * Badge — colored pill labels.
 * Based on visual-standard.md §7.6.
 */

type BadgeVariant = 'green' | 'orange' | 'red' | 'blue' | 'purple' | 'gray' | 'accent'

interface BadgeProps {
  variant?: BadgeVariant
  children: React.ReactNode
}

const variantStyles: Record<BadgeVariant, { bg: string; color: string }> = {
  green: { bg: 'var(--color-semantic-green-bg)', color: 'var(--color-semantic-green)' },
  orange: { bg: 'var(--color-semantic-orange-bg)', color: 'var(--color-semantic-orange)' },
  red: { bg: 'var(--color-semantic-red-bg)', color: 'var(--color-semantic-red)' },
  blue: { bg: 'var(--color-semantic-blue-bg)', color: 'var(--color-semantic-blue)' },
  purple: { bg: 'var(--color-semantic-purple-bg)', color: 'var(--color-semantic-purple)' },
  gray: { bg: 'var(--color-bg-neutral)', color: 'var(--color-text-muted)' },
  accent: { bg: 'var(--color-accent-subtle)', color: 'var(--color-accent)' },
}

export function Badge({ variant = 'gray', children }: BadgeProps) {
  const styles = variantStyles[variant]
  return (
    <span
      className="inline-block rounded-xl px-2.5 py-0.5 text-xs font-medium"
      style={{ background: styles.bg, color: styles.color }}
    >
      {children}
    </span>
  )
}
