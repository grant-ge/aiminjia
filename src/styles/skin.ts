import { hexToRgb, mixColors } from '@/lib/themeUtils'

export const DEFAULT_ACCENT_COLOR = '#D4A843'

export const DERIVED_SKIN_KEYS = [
  '--primary',
  '--primary-foreground',
  '--primary-rgb',
  '--ring',
  '--sidebar-primary',
  '--sidebar-primary-foreground',
  '--brand-primary-subtle',
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
  const foreground = '#FFFFFF'
  const [r, g, b] = hexToRgb(accent)

  return {
    '--primary': accent,
    '--primary-foreground': foreground,
    '--primary-rgb': `${r}, ${g}, ${b}`,
    '--ring': accent,
    '--sidebar-primary': accent,
    '--sidebar-primary-foreground': foreground,
    '--brand-primary-subtle': mixColors(accent, '#FFFFFF', 0.14),
  }
}
