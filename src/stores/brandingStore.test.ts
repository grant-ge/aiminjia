import { beforeEach, describe, expect, it } from 'vitest'

import { DEFAULTS, useBrandingStore } from '@/stores/brandingStore'

describe('brandingStore', () => {
  beforeEach(() => {
    useBrandingStore.getState().reset()
    document.documentElement.removeAttribute('style')
  })

  it('applyBranding 写入完整租户 4 色（design.pen 命名空间）', () => {
    useBrandingStore.getState().applyBranding({
      productName: '租户 A',
      accentColor: '#2563EB',
      primaryColor: '#1E293B',
      bgColor: '#F7F9FE',
      sidebarBgColor: '#EEF2FA',
    })

    const style = document.documentElement.style
    expect(style.getPropertyValue('--primary')).toBe('#2563EB')
    expect(style.getPropertyValue('--ring')).toBe('#2563EB')
    expect(style.getPropertyValue('--sidebar-primary')).toBe('#2563EB')
    expect(style.getPropertyValue('--foreground')).toBe('#1E293B')
    expect(style.getPropertyValue('--background')).toBe('#F7F9FE')
    expect(style.getPropertyValue('--sidebar')).toBe('#EEF2FA')

    const state = useBrandingStore.getState()
    expect(state.accentColor).toBe('#2563EB')
    expect(state.primaryColor).toBe('#1E293B')
    expect(state.bgColor).toBe('#F7F9FE')
    expect(state.sidebarBgColor).toBe('#EEF2FA')
    expect(state.isCustom).toBe(true)
  })

  it('applyBranding 也下发 legacy --color-* 命名空间（兼容旧组件）', () => {
    useBrandingStore.getState().applyBranding({ accentColor: '#960505', primaryColor: '#1A1A1A' })
    const style = document.documentElement.style
    expect(style.getPropertyValue('--color-accent')).toBe('#960505')
    expect(style.getPropertyValue('--color-primary')).toBe('#1A1A1A')
    expect(style.getPropertyValue('--color-text-primary')).toBe('#1A1A1A')
  })

  it('租户只下发 accent 时仍走默认 primary/bg/sidebar', () => {
    useBrandingStore.getState().applyBranding({ accentColor: '#0891B2' })
    const state = useBrandingStore.getState()
    expect(state.accentColor).toBe('#0891B2')
    expect(state.primaryColor).toBe(DEFAULTS.primaryColor)
    expect(state.bgColor).toBe(DEFAULTS.bgColor)
    expect(state.sidebarBgColor).toBe(DEFAULTS.sidebarBgColor)
  })

  it('暗夜模式（深色 bg）时 muted/border 仍按 fg 反向派生', () => {
    useBrandingStore.getState().applyBranding({
      accentColor: '#818CF8',
      primaryColor: '#E2E8F0',
      bgColor: '#0F172A',
      sidebarBgColor: '#1E293B',
    })
    const style = document.documentElement.style
    expect(style.getPropertyValue('--background')).toBe('#0F172A')
    expect(style.getPropertyValue('--foreground')).toBe('#E2E8F0')
    // muted 是 bg 与 fg 混合的中间值，不应为纯 bg 或纯 fg
    const muted = style.getPropertyValue('--muted')
    expect(muted).not.toBe('#0F172A')
    expect(muted).not.toBe('#E2E8F0')
  })

  it('非法 hex 回退默认值', () => {
    useBrandingStore.getState().applyBranding({
      accentColor: 'not-a-color',
      primaryColor: 'abc',
    })
    const state = useBrandingStore.getState()
    expect(state.accentColor).toBe(DEFAULTS.accentColor)
    expect(state.primaryColor).toBe(DEFAULTS.primaryColor)
  })

  it('租户 logoUrl 走 lotus 代理；本地 / 路径不变', () => {
    useBrandingStore.getState().applyBranding({
      logoUrl: 'https://oss.example.com/logo.png',
    })
    expect(useBrandingStore.getState().logoUrl).toBe(
      'https://ai-tenant.renlijia.com/api/file?url=' +
        encodeURIComponent('https://oss.example.com/logo.png'),
    )

    useBrandingStore.getState().reset()
    useBrandingStore.getState().applyBranding({ logoUrl: '/local-logo.svg' })
    expect(useBrandingStore.getState().logoUrl).toBe('/local-logo.svg')
  })

  it('reset 清掉所有覆盖并回退默认状态', () => {
    useBrandingStore.getState().applyBranding({
      accentColor: '#2563EB',
      primaryColor: '#1E293B',
      bgColor: '#F7F9FE',
      sidebarBgColor: '#EEF2FA',
    })
    useBrandingStore.getState().reset()

    const style = document.documentElement.style
    expect(style.getPropertyValue('--primary')).toBe('')
    expect(style.getPropertyValue('--background')).toBe('')
    expect(style.getPropertyValue('--sidebar')).toBe('')
    expect(style.getPropertyValue('--color-accent')).toBe('')

    const state = useBrandingStore.getState()
    expect(state.accentColor).toBe(DEFAULTS.accentColor)
    expect(state.primaryColor).toBe(DEFAULTS.primaryColor)
    expect(state.isCustom).toBe(false)
  })
})
