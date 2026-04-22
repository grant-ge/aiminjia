import { describe, expect, it } from 'vitest'

import { DERIVED_SKIN_KEYS, deriveSkin } from '@/styles/skin'


describe('deriveSkin', () => {
  it('从 accentColor 派生 shadcn token', () => {
    const skin = deriveSkin('#DBAA22')

    expect(skin['--primary']).toBe('#DBAA22')
    expect(skin['--ring']).toBe('#DBAA22')
    expect(skin['--sidebar']).toMatch(/^#/i)
    expect(skin['--sidebar-accent']).toMatch(/^#/i)
    expect(skin['--primary-foreground']).toBe('#1A1A1A')
    expect(Object.keys(skin)).toEqual(DERIVED_SKIN_KEYS)
  })

  it('深色主色时返回白色前景', () => {
    const skin = deriveSkin('#1A2E22')

    expect(skin['--primary-foreground']).toBe('#FFFFFF')
    expect(skin['--sidebar-primary-foreground']).toBe('#FFFFFF')
  })

  it('非法输入时回退默认金色', () => {
    const skin = deriveSkin('bad-color')

    expect(skin['--primary']).toBe('#DBAA22')
  })
})
