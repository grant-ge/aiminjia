import { describe, expect, it } from 'vitest'

import { DEFAULT_ACCENT_COLOR, DERIVED_SKIN_KEYS, deriveSkin } from './skin'

describe('deriveSkin', () => {
  it('returns only the 6 accent-bound CSS variables', () => {
    const result = deriveSkin(DEFAULT_ACCENT_COLOR)
    expect(Object.keys(result).sort()).toEqual([
      '--brand-primary-subtle',
      '--primary',
      '--primary-foreground',
      '--ring',
      '--sidebar-primary',
      '--sidebar-primary-foreground',
    ])
  })

  it('derives --brand-primary-subtle from accent via color-mix with white', () => {
    expect(deriveSkin('#DBAA22')['--brand-primary-subtle']).toBe(
      'color-mix(in srgb, #DBAA22 14%, #FFFFFF)',
    )
  })

  it('uses the given accent color for --primary / --ring / --sidebar-primary', () => {
    const result = deriveSkin('#DBAA22')
    expect(result['--primary']).toBe('#DBAA22')
    expect(result['--ring']).toBe('#DBAA22')
    expect(result['--sidebar-primary']).toBe('#DBAA22')
  })

  it('uses white foreground for dark accent colors', () => {
    const result = deriveSkin('#000000')
    expect(result['--primary-foreground']).toBe('#FFFFFF')
    expect(result['--sidebar-primary-foreground']).toBe('#FFFFFF')
  })

  it('also uses white foreground for light accent colors', () => {
    const result = deriveSkin('#FFFFFF')
    expect(result['--primary-foreground']).toBe('#FFFFFF')
    expect(result['--sidebar-primary-foreground']).toBe('#FFFFFF')
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
