/**
 * @designSource manual — chat avatar for user / assistant rows
 *
 * Renders a 28×28 circular avatar with three fallback layers:
 *   1. `src` (preferred — assistant uses `brandingStore.logoUrl`,
 *      user can supply a saved profile image)
 *   2. `variant='neutral'` (gender-free user silhouette using lucide
 *      `User` icon in brand `var(--primary)` accent — colored via the
 *      Tailwind `text-primary` class so it automatically tracks tenant
 *      brand changes through `currentColor`)
 *   3. `initial` (first non-whitespace character of `name`) painted on a
 *      deterministic color derived from the `colorSeed` (hashes the name
 *      by default)
 *
 * Used by `MessageList` to give each chat row a "two people talking"
 * feel — AI assistant on the left with product logo, user on the right
 * with the neutral brand-tinted silhouette.
 */
import { User } from 'lucide-react'

interface ChatAvatarProps {
  name: string
  src?: string | null
  /**
   * Override the default fallback. `'neutral'` paints a gender-free
   * `User` icon (lucide) in `var(--primary)` (good default for the
   * current user, since we don't store profile photos yet).
   * `'initial'` paints the first character on a palette color hashed
   * from `colorSeed` / `name` — kept for non-user contexts (e.g. a chat
   * room with multiple expert names where varied colors aid scanning).
   */
  variant?: 'initial' | 'neutral'
  /** Override the auto-derived background color seed. Defaults to `name`. */
  colorSeed?: string
  /** Visual diameter in px. Default 28. */
  size?: number
  /** Border halo color — set to make a "ring" around the avatar. */
  ringColor?: string
}

// 8 pleasant palette stops; chosen to read well on both light + dark
// backgrounds and to NOT clash with `--primary` brand accent.
const PALETTE = [
  '#FB7185', // rose
  '#FB923C', // orange
  '#FACC15', // yellow
  '#34D399', // emerald
  '#22D3EE', // cyan
  '#60A5FA', // blue
  '#A78BFA', // violet
  '#F472B6', // pink
] as const

function hashString(s: string): number {
  let h = 0
  for (let i = 0; i < s.length; i++) {
    h = (h * 31 + s.charCodeAt(i)) | 0
  }
  return Math.abs(h)
}

function pickColor(seed: string): string {
  if (!seed) return PALETTE[0]
  return PALETTE[hashString(seed) % PALETTE.length]
}

function firstInitial(name: string): string {
  const trimmed = (name ?? '').trim()
  if (!trimmed) return '·'
  // Use Array.from to grab the first code point (handles emoji + CJK).
  return Array.from(trimmed)[0]?.toUpperCase() ?? '·'
}

export function ChatAvatar({
  name,
  src,
  variant = 'initial',
  colorSeed,
  size = 28,
  ringColor,
}: ChatAvatarProps) {
  // Treat empty string the same as null/undefined. brandingStore.logoUrl
  // is `''` when a tenant explicitly cleared its logo, and we still want
  // to fall through to the variant fallback (neutral icon / initial) in
  // that case — not render an <img src="">.
  const normalizedSrc = src && src.length > 0 ? src : null
  const usingImage = normalizedSrc !== null
  const usingNeutralFallback = !usingImage && variant === 'neutral'
  const initial = firstInitial(name)
  // Neutral variant: container fills with `--primary` at ~12% alpha (subtle
  // accent halo) and the lucide `User` icon paints itself with the full
  // `--primary` via `text-primary` (currentColor). Tenant accent swaps
  // automatically through the CSS variable — no JS wiring needed.
  const dataVariant = usingImage ? 'image' : usingNeutralFallback ? 'neutral' : 'initial'
  const bg = usingImage
    ? 'transparent'
    : usingNeutralFallback
      ? 'color-mix(in srgb, var(--primary) 12%, transparent)'
      : pickColor(colorSeed ?? name)
  const style: React.CSSProperties = {
    width: size,
    height: size,
    background: bg,
    boxShadow: ringColor ? `0 0 0 2px ${ringColor}` : undefined,
  }
  return (
    <span
      data-testid="chat-avatar"
      data-variant={dataVariant}
      aria-label={name}
      title={name}
      style={style}
      className="inline-flex shrink-0 select-none items-center justify-center overflow-hidden rounded-full text-xs font-semibold text-white"
    >
      {usingImage ? (
        <img
          src={normalizedSrc}
          alt=""
          width={size}
          height={size}
          className="h-full w-full object-cover"
          // Hide broken images — the wrapper falls back to its colored bg.
          onError={(e) => {
            ;(e.currentTarget as HTMLImageElement).style.display = 'none'
          }}
        />
      ) : usingNeutralFallback ? (
        <User
          aria-hidden
          className="text-primary"
          // Icon occupies ~60% of the avatar so the bg halo reads as a ring
          style={{ width: size * 0.6, height: size * 0.6 }}
          strokeWidth={2.25}
        />
      ) : (
        <span aria-hidden>{initial}</span>
      )}
    </span>
  )
}
