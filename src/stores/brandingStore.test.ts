import { beforeEach, describe, expect, it } from 'vitest'

import { DEFAULTS, useBrandingStore } from '@/stores/brandingStore'
import { DERIVED_SKIN_KEYS } from '@/styles/skin'

describe('brandingStore', () => {
  beforeEach(() => {
    useBrandingStore.getState().reset()
    document.documentElement.removeAttribute('style')
  })

  it('applyBranding 仅使用 accentColor 派生 token', () => {
    useBrandingStore.getState().applyBranding({
      productName: '租户 A',
      accentColor: '#960505',
      primaryColor: '#123456',
      bgColor: '#eeeeee',
      sidebarBgColor: '#cccccc',
    })

    expect(document.documentElement.style.getPropertyValue('--primary')).toBe('#960505')
    expect(document.documentElement.style.getPropertyValue('--sidebar')).toBe('')
    expect(document.documentElement.style.getPropertyValue('--sidebar-accent')).toBe('')
    expect(useBrandingStore.getState().accentColor).toBe('#960505')
    expect(useBrandingStore.getState().isCustom).toBe(true)
  })

  it('reset 会移除全部派生变量并回退默认值', () => {
    useBrandingStore.getState().applyBranding({ accentColor: '#1A2E22' })
    useBrandingStore.getState().reset()

    for (const key of DERIVED_SKIN_KEYS) {
      expect(document.documentElement.style.getPropertyValue(key)).toBe('')
    }
    expect(useBrandingStore.getState().productName).toBe(DEFAULTS.productName)
    expect(useBrandingStore.getState().accentColor).toBe(DEFAULTS.accentColor)
  })

  it('非法 accentColor 会回退默认色并保持状态一致', () => {
    useBrandingStore.getState().applyBranding({ accentColor: 'abc' })

    expect(document.documentElement.style.getPropertyValue('--primary')).toBe(DEFAULTS.accentColor)
    expect(useBrandingStore.getState().accentColor).toBe(DEFAULTS.accentColor)
    expect(useBrandingStore.getState().isCustom).toBe(false)
  })
})
