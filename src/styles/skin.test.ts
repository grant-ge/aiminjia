import { describe, expect, it } from 'vitest'

import { DEFAULT_ACCENT_COLOR, DERIVED_SKIN_KEYS, deriveSkin } from './skin'

describe('deriveSkin', () => {
  it('returns only the 5 accent-bound CSS variables', () => {
    const result = deriveSkin(DEFAULT_ACCENT_COLOR)
    expect(Object.keys(result).sort()).toEqual([
      '--primary',
      '--primary-foreground',
      '--ring',
      '--sidebar-primary',
      '--sidebar-primary-foreground',
    ])
  })

  it('uses the given accent color for --primary / --ring / --sidebar-primary', () => {
    const result = deriveSkin('#DBAA22')
    expect(result['--primary']).toBe('#DBAA22')
    expect(result['--ring']).toBe('#DBAA22')
    expect(result['--sidebar-primary']).toBe('#DBAA22')
  })

  it('chooses white foreground for dark accent colors', () => {
    const result = deriveSkin('#000000')
    expect(result['--primary-foreground']).toBe('#FFFFFF')
    expect(result['--sidebar-primary-foreground']).toBe('#FFFFFF')
  })

  it('chooses near-black foreground for light accent colors', () => {
    const result = deriveSkin('#FFFFFF')
    expect(result['--primary-foreground']).toBe('#1A1A1A')
    expect(result['--sidebar-primary-foreground']).toBe('#1A1A1A')
  })

  it('falls back to default accent color when input is invalid', () => {
    expect(deriveSkin('not-a-color')['--primary']).toBe(DEFAULT_ACCENT_COLOR)
    expect(deriveSkin(undefined)['--primary']).toBe(DEFAULT_ACCENT_COLOR)
  })

  it('exports DERIVED_SKIN_KEYS matching the result keys', () => {
    const result = deriveSkin(DEFAULT_ACCENT_COLOR)
    expect([...DERIVED_SKIN_KEYS].sort()).toEqual(Object.keys(result).sort())
  })
})
