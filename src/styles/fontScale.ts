import type { FontScale } from '@/types/settings'

export const FONT_SCALE_ROOT_PX: Record<FontScale, number> = {
  small: 14.7692307692,
  medium: 16,
  large: 17.2307692308,
}

export const FONT_SCALE_STORAGE_KEY = 'aijia-font-scale'
export const DEFAULT_FONT_SCALE: FontScale = 'small'

export function normalizeFontScale(value: unknown): FontScale {
  return value === 'small' || value === 'large' || value === 'medium' ? value : DEFAULT_FONT_SCALE
}

export function applyFontScale(value: FontScale) {
  if (typeof document === 'undefined') return
  const scale = normalizeFontScale(value)
  document.documentElement.style.fontSize = `${FONT_SCALE_ROOT_PX[scale]}px`
}

export function loadPersistedFontScale(): FontScale {
  if (typeof localStorage === 'undefined') return DEFAULT_FONT_SCALE
  try {
    return normalizeFontScale(localStorage.getItem(FONT_SCALE_STORAGE_KEY))
  } catch {
    return DEFAULT_FONT_SCALE
  }
}

export function persistFontScale(value: FontScale) {
  if (typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(FONT_SCALE_STORAGE_KEY, normalizeFontScale(value))
  } catch {
    // Ignore storage failures; the in-memory setting still applies.
  }
}
