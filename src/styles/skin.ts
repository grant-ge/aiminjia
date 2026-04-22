import { isDarkColor } from '@/lib/themeUtils'

export const DEFAULT_ACCENT_COLOR = '#DBAA22'

export const DERIVED_SKIN_KEYS = [
  '--primary',
  '--primary-foreground',
  '--ring',
  '--sidebar-primary',
  '--sidebar-primary-foreground',
] as const

function normalizeAccentColor(input?: string): string {
  return /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(input ?? '')
    ? (input as string)
    : DEFAULT_ACCENT_COLOR
}

export function deriveSkin(
  accentColor?: string,
): Record<(typeof DERIVED_SKIN_KEYS)[number], string> {
  const accent = normalizeAccentColor(accentColor)
  const foreground = isDarkColor(accent) ? '#FFFFFF' : '#1A1A1A'

  return {
    '--primary': accent,
    '--primary-foreground': foreground,
    '--ring': accent,
    '--sidebar-primary': accent,
    '--sidebar-primary-foreground': foreground,
  }
}
