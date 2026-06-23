import { afterEach, describe, expect, it } from 'vitest'

import {
  applyFontScale,
  FONT_SCALE_ROOT_PX,
  FONT_SCALE_STORAGE_KEY,
  loadPersistedFontScale,
  persistFontScale,
} from './fontScale'

describe('fontScale', () => {
  afterEach(() => {
    document.documentElement.style.fontSize = ''
    localStorage.clear()
  })

  it('maps small, medium, and large to root pixel baselines', () => {
    expect(FONT_SCALE_ROOT_PX).toEqual({ small: 14.7692307692, medium: 16, large: 17.2307692308 })
  })

  it('applies the selected scale to the root element', () => {
    applyFontScale('large')
    expect(document.documentElement.style.fontSize).toBe('17.2307692308px')

    applyFontScale('small')
    expect(document.documentElement.style.fontSize).toBe('14.7692307692px')
  })

  it('falls back to medium for unknown persisted values', () => {
    applyFontScale('huge' as never)
    expect(document.documentElement.style.fontSize).toBe('16px')
  })

  it('persists and loads the selected font scale', () => {
    expect(loadPersistedFontScale()).toBe('medium')

    persistFontScale('small')
    expect(localStorage.getItem(FONT_SCALE_STORAGE_KEY)).toBe('small')
    expect(loadPersistedFontScale()).toBe('small')

    localStorage.setItem(FONT_SCALE_STORAGE_KEY, 'giant')
    expect(loadPersistedFontScale()).toBe('medium')
  })
})
