import { beforeEach, describe, expect, it } from 'vitest'

import { useBrandingStore } from '@/stores/brandingStore'

describe('brandingStore (plan-A token slimming)', () => {
  beforeEach(() => {
    useBrandingStore.getState().reset()
  })

  it('applyBranding writes ONLY the 5 accent-bound CSS vars to documentElement', () => {
    useBrandingStore.getState().applyBranding({ accentColor: '#DBAA22' })
    const style = document.documentElement.style
    expect(style.getPropertyValue('--primary').trim()).toBe('#DBAA22')
    expect(style.getPropertyValue('--ring').trim()).toBe('#DBAA22')
    expect(style.getPropertyValue('--sidebar-primary').trim()).toBe('#DBAA22')
    // sidebar 与 sidebar-accent 由 globals.css 提供静态值，运行时不被 branding 改写
    expect(style.getPropertyValue('--sidebar').trim()).toBe('')
    expect(style.getPropertyValue('--sidebar-accent').trim()).toBe('')
  })

  it('reset clears the 5 accent-bound vars', () => {
    useBrandingStore.getState().applyBranding({ accentColor: '#FF0000' })
    useBrandingStore.getState().reset()
    const style = document.documentElement.style
    expect(style.getPropertyValue('--primary')).toBe('')
    expect(style.getPropertyValue('--sidebar-primary')).toBe('')
  })
})
