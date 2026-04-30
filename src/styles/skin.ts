export const DEFAULT_ACCENT_COLOR = '#DBAA22'

export const DERIVED_SKIN_KEYS = [
  '--primary',
  '--primary-foreground',
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

  return {
    '--primary': accent,
    '--primary-foreground': foreground,
    '--ring': accent,
    '--sidebar-primary': accent,
    '--sidebar-primary-foreground': foreground,
    '--brand-primary-subtle': `color-mix(in srgb, ${accent} 14%, #FFFFFF)`,
  }
}
