/**
 * Agent visual identity: derive a stable color slot and avatar text from an
 * agent name. Lead is special-cased to use the primary brand color; teammates
 * cycle through a fixed palette of theme-friendly hue slots.
 *
 * The palette uses HSL-relative-to-foreground combinations so it survives
 * dark/light theme switching without per-mode overrides.
 */

const LEAD_NAMES = new Set(['team-lead', 'lead', '负责人'])

interface AgentIdentityStyle {
  /** Tailwind classes for the small avatar circle / chip. */
  avatarClass: string
  /** Tailwind classes for an agent's bubble background. */
  bubbleClass: string
  /** Tailwind classes for accent dot / inline marker. */
  accentClass: string
  /** Short label shown inside the avatar (1-2 chars). */
  initials: string
}

/**
 * Hash a string to a stable small int. Not cryptographically anything —
 * we just need consistent slot assignment across renders.
 */
function hashStr(str: string): number {
  let h = 0
  for (let i = 0; i < str.length; i++) {
    h = (h * 31 + str.charCodeAt(i)) | 0
  }
  return Math.abs(h)
}

/**
 * Palette: each slot pairs (bubble bg, accent dot, avatar bg/text).
 * Color values use theme variables so dark/light mode just works.
 * The avatar uses solid bg + foreground text in inverted contrast.
 */
const PALETTE: Array<Omit<AgentIdentityStyle, 'initials'>> = [
  {
    avatarClass: 'bg-blue-500/15 text-blue-700 dark:text-blue-300 ring-1 ring-blue-500/30',
    bubbleClass: 'bg-blue-500/8 border border-blue-500/20',
    accentClass: 'bg-blue-500',
  },
  {
    avatarClass: 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-300 ring-1 ring-emerald-500/30',
    bubbleClass: 'bg-emerald-500/8 border border-emerald-500/20',
    accentClass: 'bg-emerald-500',
  },
  {
    avatarClass: 'bg-rose-500/15 text-rose-700 dark:text-rose-300 ring-1 ring-rose-500/30',
    bubbleClass: 'bg-rose-500/8 border border-rose-500/20',
    accentClass: 'bg-rose-500',
  },
  {
    avatarClass: 'bg-amber-500/15 text-amber-700 dark:text-amber-300 ring-1 ring-amber-500/30',
    bubbleClass: 'bg-amber-500/8 border border-amber-500/20',
    accentClass: 'bg-amber-500',
  },
  {
    avatarClass: 'bg-violet-500/15 text-violet-700 dark:text-violet-300 ring-1 ring-violet-500/30',
    bubbleClass: 'bg-violet-500/8 border border-violet-500/20',
    accentClass: 'bg-violet-500',
  },
  {
    avatarClass: 'bg-cyan-500/15 text-cyan-700 dark:text-cyan-300 ring-1 ring-cyan-500/30',
    bubbleClass: 'bg-cyan-500/8 border border-cyan-500/20',
    accentClass: 'bg-cyan-500',
  },
]

const LEAD_STYLE: Omit<AgentIdentityStyle, 'initials'> = {
  avatarClass: 'bg-primary text-primary-foreground ring-1 ring-primary/40',
  bubbleClass: 'bg-primary/10 border border-primary/25',
  accentClass: 'bg-primary',
}

function deriveInitials(name: string): string {
  const trimmed = name.trim()
  if (!trimmed) return '?'
  // Single ASCII word — take first char.
  if (/^[A-Za-z]+$/.test(trimmed)) {
    return trimmed.slice(0, 2).toUpperCase()
  }
  // Multi-char CJK — take first char only (it's already one glyph).
  return trimmed.slice(0, Math.min(2, trimmed.length))
}

export function getAgentIdentity(name: string): AgentIdentityStyle {
  const initials = deriveInitials(name)
  if (LEAD_NAMES.has(name)) {
    return { ...LEAD_STYLE, initials }
  }
  const slot = hashStr(name) % PALETTE.length
  return { ...PALETTE[slot], initials }
}

export function isLeadName(name: string): boolean {
  return LEAD_NAMES.has(name)
}

export function formatLeadDisplayName(name: string): string {
  if (LEAD_NAMES.has(name)) return 'Lead'
  return name
}
