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
    expect(FONT_SCALE_ROOT_PX).toEqual({ small: 14, medium: 16, large: 18 })
  })

  it('applies the selected scale to the root element', () => {
    applyFontScale('large')
    expect(document.documentElement.style.fontSize).toBe('18px')

    applyFontScale('small')
    expect(document.documentElement.style.fontSize).toBe('14px')
  })

  it('falls back to small for unknown persisted values', () => {
    applyFontScale('huge' as never)
    expect(document.documentElement.style.fontSize).toBe('14px')
  })

  it('persists and loads the selected font scale', () => {
    expect(loadPersistedFontScale()).toBe('small')

    persistFontScale('small')
    expect(localStorage.getItem(FONT_SCALE_STORAGE_KEY)).toBe('small')
    expect(loadPersistedFontScale()).toBe('small')

    localStorage.setItem(FONT_SCALE_STORAGE_KEY, 'giant')
    expect(loadPersistedFontScale()).toBe('small')
  })
})
