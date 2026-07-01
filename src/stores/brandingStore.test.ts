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
    expect(style.getPropertyValue('--background')).toBe('#FFFFFF')
    expect(style.getPropertyValue('--card')).toBe('#FFFFFF')
    expect(style.getPropertyValue('--popover')).toBe('#FFFFFF')
    expect(style.getPropertyValue('--sidebar')).toBe('#EEF2FA')

    const state = useBrandingStore.getState()
    expect(state.accentColor).toBe('#2563EB')
    expect(state.primaryColor).toBe('#1E293B')
    expect(state.bgColor).toBe(DEFAULTS.bgColor)
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

  it('浅色品牌金也保持 primary foreground 为白色', () => {
    useBrandingStore.getState().applyBranding({ accentColor: '#D4A843' })

    const style = document.documentElement.style
    expect(style.getPropertyValue('--primary')).toBe('#D4A843')
    expect(style.getPropertyValue('--primary-foreground')).toBe('#FFFFFF')
    expect(style.getPropertyValue('--sidebar-primary-foreground')).toBe('#FFFFFF')
    expect(style.getPropertyValue('--color-text-on-accent')).toBe('#FFFFFF')
  })

  it('租户 bgColor 只作为侧边栏底色来源，不影响主内容 bg-background', () => {
    useBrandingStore.getState().applyBranding({
      accentColor: '#818CF8',
      primaryColor: '#E2E8F0',
      bgColor: '#0F172A',
    })
    const style = document.documentElement.style
    expect(style.getPropertyValue('--background')).toBe('#FFFFFF')
    expect(style.getPropertyValue('--card')).toBe('#FFFFFF')
    expect(style.getPropertyValue('--popover')).toBe('#FFFFFF')
    expect(style.getPropertyValue('--sidebar')).toBe('#0F172A')
    expect(style.getPropertyValue('--foreground')).toBe('#E2E8F0')
    // muted 是主内容白底与 fg 混合的中间值，不应为纯白或纯 fg
    const muted = style.getPropertyValue('--muted')
    expect(muted).not.toBe('#FFFFFF')
    expect(muted).not.toBe('#E2E8F0')
  })

  it('租户 sidebarBgColor 优先于兼容的 bgColor 作为侧边栏底色', () => {
    useBrandingStore.getState().applyBranding({
      bgColor: '#F7F9FE',
      sidebarBgColor: '#EEF2FA',
    })

    const style = document.documentElement.style
    expect(style.getPropertyValue('--background')).toBe('#FFFFFF')
    expect(style.getPropertyValue('--sidebar')).toBe('#EEF2FA')
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

  it('applyBranding 同步写入所有 Safari 13 compat RGB 变量和跨 palette 派生变量', () => {
    useBrandingStore.getState().applyBranding({
      accentColor: '#2563EB',
      primaryColor: '#1E293B',
      bgColor: '#F7F9FE',
    })
    const style = document.documentElement.style
    // RGB companion vars — exact values
    expect(style.getPropertyValue('--primary-rgb')).toBe('37, 99, 235')
    expect(style.getPropertyValue('--primary-foreground-rgb')).toBe('255, 255, 255')
    expect(style.getPropertyValue('--foreground-rgb')).toBe('30, 41, 59')
    expect(style.getPropertyValue('--sidebar-foreground-rgb')).toBe('30, 41, 59')
    expect(style.getPropertyValue('--background-rgb')).toBe('255, 255, 255')
    expect(style.getPropertyValue('--card-rgb')).toBe('255, 255, 255')
    expect(style.getPropertyValue('--popover-rgb')).toBe('255, 255, 255')
    expect(style.getPropertyValue('--secondary-rgb')).toBe('244, 244, 245')
    expect(style.getPropertyValue('--accent-rgb')).toBe('244, 244, 245')
    expect(style.getPropertyValue('--muted-rgb')).toBe('242, 242, 243')
    expect(style.getPropertyValue('--border-rgb')).toBe('228, 229, 231')
    expect(style.getPropertyValue('--input-rgb')).toBe('228, 229, 231')
    expect(style.getPropertyValue('--sidebar-accent-rgb')).toBe('230, 232, 238')
    expect(style.getPropertyValue('--color-bg-base-rgb')).toBe('246, 246, 247')
    expect(style.getPropertyValue('--muted-foreground-rgb')).toBe('131, 137, 147')
    // cross-palette vars — exact values
    expect(style.getPropertyValue('--primary-on-bg-10')).toBe('#e9effd')
    expect(style.getPropertyValue('--primary-on-bg-24')).toBe('#cbdafa')
    expect(style.getPropertyValue('--primary-on-bg-72')).toBe('#628ff1')
    expect(style.getPropertyValue('--primary-darken-10')).toBe('#2159d4')
    expect(style.getPropertyValue('--primary-mix-scrollbar')).toBe('#98b1e9')
    expect(style.getPropertyValue('--primary-mix-blockquote')).toBe('#9bb4e9')
  })

  it('reset 清除所有 RGB 和跨 palette 派生变量', () => {
    useBrandingStore.getState().applyBranding({ accentColor: '#2563EB', bgColor: '#F7F9FE' })
    useBrandingStore.getState().reset()
    const style = document.documentElement.style
    expect(style.getPropertyValue('--primary-rgb')).toBe('')
    expect(style.getPropertyValue('--primary-foreground-rgb')).toBe('')
    expect(style.getPropertyValue('--foreground-rgb')).toBe('')
    expect(style.getPropertyValue('--sidebar-foreground-rgb')).toBe('')
    expect(style.getPropertyValue('--background-rgb')).toBe('')
    expect(style.getPropertyValue('--muted-rgb')).toBe('')
    expect(style.getPropertyValue('--card-rgb')).toBe('')
    expect(style.getPropertyValue('--popover-rgb')).toBe('')
    expect(style.getPropertyValue('--secondary-rgb')).toBe('')
    expect(style.getPropertyValue('--accent-rgb')).toBe('')
    expect(style.getPropertyValue('--border-rgb')).toBe('')
    expect(style.getPropertyValue('--input-rgb')).toBe('')
    expect(style.getPropertyValue('--sidebar-accent-rgb')).toBe('')
    expect(style.getPropertyValue('--muted-foreground-rgb')).toBe('')
    expect(style.getPropertyValue('--color-bg-base-rgb')).toBe('')
    expect(style.getPropertyValue('--primary-on-bg-10')).toBe('')
    expect(style.getPropertyValue('--primary-on-bg-24')).toBe('')
    expect(style.getPropertyValue('--primary-on-bg-72')).toBe('')
    expect(style.getPropertyValue('--primary-darken-10')).toBe('')
    expect(style.getPropertyValue('--primary-mix-scrollbar')).toBe('')
    expect(style.getPropertyValue('--primary-mix-blockquote')).toBe('')
  })

  it('applyBranding({}) 后默认主题 CSS 变量与 globals.css 静态默认值一致', () => {
    useBrandingStore.getState().applyBranding({})
    const style = document.documentElement.style
    expect(style.getPropertyValue('--primary')).toBe(DEFAULTS.accentColor)
    expect(style.getPropertyValue('--primary-rgb')).toBe('212, 168, 67')
    expect(style.getPropertyValue('--brand-primary-subtle')).toBe('#f9f3e5')
  })
})
