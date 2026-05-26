import { describe, expect, it } from 'vitest'

import { DEFAULT_ACCENT_COLOR, DERIVED_SKIN_KEYS, deriveSkin } from './skin'

describe('deriveSkin', () => {
  it('returns only the 7 accent-bound CSS variables', () => {
    const result = deriveSkin(DEFAULT_ACCENT_COLOR)
    expect(Object.keys(result).sort()).toEqual([
      '--brand-primary-subtle',
      '--primary',
      '--primary-foreground',
      '--primary-rgb',
      '--ring',
      '--sidebar-primary',
      '--sidebar-primary-foreground',
    ])
  })

  it('derives --brand-primary-subtle from accent mixed with white (14% accent)', () => {
    // #D4A843 14% + #FFFFFF 86% = #f9f3e5
    expect(deriveSkin('#D4A843')['--brand-primary-subtle']).toBe('#f9f3e5')
  })

  it('derives --primary-rgb as comma-separated R, G, B components', () => {
    // #D4A843 = rgb(212, 168, 67)
    expect(deriveSkin('#D4A843')['--primary-rgb']).toBe('212, 168, 67')
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
