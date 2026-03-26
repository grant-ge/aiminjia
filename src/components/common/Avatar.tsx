/**
 * Avatar — AI product icon or gender-neutral user silhouette.
 */
import { useBrandingStore } from '@/stores/brandingStore'

interface AvatarProps {
  variant: 'ai' | 'user'
  label?: string
  /** Whether the user is logged in (only for user variant) */
  isLoggedIn?: boolean
}

export function Avatar({ variant, isLoggedIn = false }: AvatarProps) {
  const isAI = variant === 'ai'
  const logoUrl = useBrandingStore((s) => s.logoUrl)
  const productName = useBrandingStore((s) => s.productName)

  if (isAI) {
    return (
      <img
        src={logoUrl}
        alt={productName}
        className="h-7 w-7 shrink-0 rounded-full"
        onError={(e) => {
          if (e.currentTarget.src !== window.location.origin + '/app-icon.png') {
            e.currentTarget.src = '/app-icon.png'
          }
        }}
      />
    )
  }

  // User avatar: carbon black when not logged in, default color when logged in
  const bgColor = isLoggedIn ? 'var(--color-user-avatar)' : '#2c2c2c'

  return (
    <div
      className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full"
      style={{ background: bgColor }}
    >
      <svg
        viewBox="0 0 24 24"
        fill="none"
        className="h-4.5 w-4.5"
      >
        <circle cx="12" cy="8" r="4" fill="#fff" />
        <path
          d="M4 20c0-3.3 3.6-6 8-6s8 2.7 8 6"
          stroke="#fff"
          strokeWidth="2"
          strokeLinecap="round"
          fill="none"
        />
      </svg>
    </div>
  )
}
