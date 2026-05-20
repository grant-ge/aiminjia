/**
 * brandingStore — runtime brand customization from tenant config.
 *
 * 照搬 v0.3.37 实现，token 命名空间映射到当前 design.pen 架构（Tailwind 4 + shadcn）：
 *   - tenant.accentColor   → --primary, --ring, --sidebar-primary, --brand-primary-subtle
 *                            (+ legacy --color-accent-* 全套，兼容旧组件)
 *   - tenant.primaryColor  → --foreground, --sidebar-foreground (+ legacy --color-primary-*)
 *   - tenant.bgColor       → --background, --card, --popover    (+ legacy --color-bg-main)
 *   - tenant.sidebarBgColor→ --sidebar                          (+ legacy --color-bg-sidebar)
 *
 * 派生：基于 4 色用 themeUtils 算 hover/active/subtle/muted 等状态色。
 * 默认值：和 lotus 后台 PRESET_THEMES[0]「默认（暖金）」完全对齐。
 */
import i18n from '@/i18n'
import { create } from 'zustand'

import { getLastBrand, saveLastBrand, type BrandSnapshot } from '@/lib/tauri'
import { darken, isDarkColor, lighten, mixColors, rgba } from '@/lib/themeUtils'

export const DEFAULTS = {
  productName: 'AI小家',
  productNameEn: 'AIjia',
  logoUrl: '/brand-avatar-gold.svg',
  accentColor: '#D4A843',
  primaryColor: '#1D1D1F',
  bgColor: '#FAFAF8',
  sidebarBgColor: '#F5F4F1',
  fontFamily: '',
}

const FONT_MAP: Record<string, string> = {
  '': '-apple-system, BlinkMacSystemFont, "PingFang SC", "Microsoft YaHei", "Segoe UI", Roboto, sans-serif',
  system: '-apple-system, BlinkMacSystemFont, "PingFang SC", "Microsoft YaHei", "Segoe UI", Roboto, sans-serif',
  kai: 'STKaiti, KaiTi, "华文楷体", serif',
  mono: 'Menlo, Consolas, Monaco, "SF Mono", "Courier New", monospace',
}

interface TenantBranding {
  productName?: string
  logoUrl?: string
  accentColor?: string
  primaryColor?: string
  bgColor?: string
  sidebarBgColor?: string
  fontFamily?: string
}

interface BrandingState {
  productName: string
  productNameEn: string
  logoUrl: string
  accentColor: string
  primaryColor: string
  bgColor: string
  sidebarBgColor: string
  fontFamily: string
  isCustom: boolean

  applyBranding(tenant: TenantBranding | null): void
  reset(): void
  /**
   * Load the cached brand from disk (`~/.renlijia/users/{scope}/brand.json`)
   * and apply it. No-op when no cache exists. Used by the login page so a
   * returning user sees the last tenant's brand before re-entering credentials.
   */
  restoreFromDisk(): Promise<void>
}

// ---------- CSS variable helpers ----------

function setVar(name: string, value: string) {
  document.documentElement.style.setProperty(name, value)
}

function removeVar(name: string) {
  document.documentElement.style.removeProperty(name)
}

function hasValue(s?: string | null): s is string {
  return !!s && s.trim().length > 0
}

function isValidHex(input?: string): input is string {
  return !!input && /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(input)
}

function normalizeHex(input: string | undefined, fallback: string): string {
  return isValidHex(input) ? input : fallback
}

// ---------- Palette derivation ----------

/**
 * Accent → 强调色一族（按钮主色、链接、选中态、ring）
 * 同时下发 design.pen 新命名（--primary 等）和 legacy --color-accent-*。
 */
function deriveAccentPalette(accent: string) {
  const onAccent = '#FFFFFF'

  // === design.pen / shadcn 命名空间 ===
  setVar('--primary', accent)
  setVar('--primary-foreground', onAccent)
  setVar('--ring', accent)
  setVar('--sidebar-primary', accent)
  setVar('--sidebar-primary-foreground', onAccent)
  setVar('--brand-primary-subtle', mixColors(accent, '#FFFFFF', 0.14))

  // === legacy --color-accent-* (53 处旧组件还在消费) ===
  setVar('--color-accent', accent)
  setVar('--color-accent-hover', darken(accent, 0.1))
  setVar('--color-accent-active', darken(accent, 0.2))
  setVar('--color-accent-light', lighten(accent, 0.4))
  setVar('--color-accent-50', lighten(accent, 0.9))
  setVar('--color-accent-100', lighten(accent, 0.75))
  setVar('--color-accent-200', lighten(accent, 0.55))
  setVar('--color-accent-300', lighten(accent, 0.35))
  setVar('--color-accent-400', accent)
  setVar('--color-accent-500', darken(accent, 0.1))
  setVar('--color-accent-600', darken(accent, 0.2))
  setVar('--color-accent-700', darken(accent, 0.35))
  setVar('--color-accent-subtle', rgba(accent, 0.12))
  setVar('--color-accent-muted', rgba(accent, 0.25))
  setVar('--color-accent-bg-light', rgba(accent, 0.04))
  setVar('--color-accent-border', rgba(accent, 0.25))
  setVar('--color-text-on-accent', onAccent)
}

/**
 * Primary → 主文本 / 重要标题色（不同于 accent，accent 用作点缀，primary 用作正文）
 * 在新架构里 primary 对应 --foreground / --sidebar-foreground。
 */
function derivePrimaryPalette(primary: string) {
  const dark = isDarkColor(primary)
  const onPrimary = dark ? '#FFFFFF' : '#1A1A1A'

  // === design.pen 命名空间：primary 是文字 / 重要内容色 ===
  setVar('--foreground', primary)
  setVar('--sidebar-foreground', primary)
  setVar('--card-foreground', primary)
  setVar('--popover-foreground', primary)

  // === legacy --color-primary-* / --color-text-* ===
  setVar('--color-primary', primary)
  setVar('--color-primary-hover', dark ? lighten(primary, 0.15) : darken(primary, 0.1))
  setVar('--color-primary-active', dark ? lighten(primary, 0.25) : darken(primary, 0.2))
  setVar('--color-text-on-primary', onPrimary)
  setVar('--color-primary-subtle', rgba(primary, 0.08))
  setVar('--color-primary-muted', rgba(primary, 0.15))
  setVar('--color-text-primary', primary)
  setVar('--color-text-secondary', rgba(primary, 0.65))
  setVar('--color-text-muted', rgba(primary, 0.45))
}

/**
 * Background → 主页面 + 卡片 + popover 底色
 * 派生 muted / border / input：在 bg 与 fg 之间按比例混合。
 */
function deriveBackgroundPalette(bg: string, fg: string) {
  setVar('--background', bg)
  setVar('--card', bg)
  setVar('--popover', bg)
  setVar('--secondary', mixColors(bg, fg, 0.95))
  setVar('--accent', mixColors(bg, fg, 0.95))
  setVar('--muted', mixColors(bg, fg, 0.94))
  setVar('--muted-foreground', mixColors(bg, fg, 0.45))
  setVar('--border', mixColors(bg, fg, 0.88))
  setVar('--input', mixColors(bg, fg, 0.88))

  // === legacy ===
  setVar('--color-bg-main', bg)
  setVar('--color-bg-card', bg)
  setVar('--color-bg-elevated', bg)
  setVar('--color-bg-card-hover', mixColors(bg, fg, 0.94))
  setVar('--color-bg-input', mixColors(bg, fg, 0.88))
  setVar('--color-border', mixColors(bg, fg, 0.88))
  setVar('--color-border-light', mixColors(bg, fg, 0.92))
  setVar('--color-border-subtle', mixColors(bg, fg, 0.92))
  setVar('--color-bg-base', mixColors(bg, fg, 0.96))
  setVar('--color-bg-msg-user', mixColors(bg, fg, 0.94))
  setVar('--color-bg-code', mixColors(bg, fg, 0.94))
  setVar('--color-bg-code-header', bg)
}

/**
 * Sidebar → 侧栏底色，独立派生 sidebar-accent / sidebar-border。
 */
function deriveSidebarPalette(sidebarBg: string, fg: string) {
  setVar('--sidebar', sidebarBg)
  setVar('--sidebar-accent', mixColors(sidebarBg, fg, 0.92))
  setVar('--sidebar-accent-foreground', fg)
  setVar('--sidebar-border', mixColors(sidebarBg, fg, 0.9))
  setVar('--sidebar-ring', mixColors(sidebarBg, fg, 0.6))

  // === legacy ===
  setVar('--color-bg-sidebar', sidebarBg)
  setVar('--color-bg-sidebar-hover', mixColors(sidebarBg, fg, 0.9))
}

// ---------- 全集，用于 reset ----------

const ALL_OVERRIDDEN_VARS = [
  // design.pen / shadcn
  '--primary', '--primary-foreground', '--ring', '--sidebar-primary', '--sidebar-primary-foreground',
  '--brand-primary-subtle',
  '--foreground', '--sidebar-foreground', '--card-foreground', '--popover-foreground',
  '--background', '--card', '--popover', '--secondary', '--accent', '--muted', '--muted-foreground',
  '--border', '--input',
  '--sidebar', '--sidebar-accent', '--sidebar-accent-foreground', '--sidebar-border', '--sidebar-ring',
  '--font-sans',
  // legacy --color-accent-*
  '--color-accent', '--color-accent-hover', '--color-accent-active', '--color-accent-light',
  '--color-accent-50', '--color-accent-100', '--color-accent-200', '--color-accent-300',
  '--color-accent-400', '--color-accent-500', '--color-accent-600', '--color-accent-700',
  '--color-accent-subtle', '--color-accent-muted', '--color-accent-bg-light', '--color-accent-border',
  '--color-text-on-accent',
  // legacy --color-primary-* / --color-text-*
  '--color-primary', '--color-primary-hover', '--color-primary-active',
  '--color-text-on-primary', '--color-primary-subtle', '--color-primary-muted',
  '--color-text-primary', '--color-text-secondary', '--color-text-muted',
  // legacy bg / border
  '--color-bg-main', '--color-bg-card', '--color-bg-elevated', '--color-bg-card-hover',
  '--color-bg-input', '--color-bg-base', '--color-bg-msg-user', '--color-bg-code', '--color-bg-code-header',
  '--color-border', '--color-border-light', '--color-border-subtle',
  '--color-bg-sidebar', '--color-bg-sidebar-hover',
]

// ---------- Logo URL 代理（OSS 远端 URL 走 lotus /api/file 代理） ----------

function resolveLogoUrl(raw: string): string {
  if (!raw || raw.startsWith('/')) return raw
  try {
    const u = new URL(raw)
    if (u.protocol === 'http:' || u.protocol === 'https:') {
      return `https://ai-tenant.renlijia.com/api/file?url=${encodeURIComponent(raw)}`
    }
  } catch {
    /* ignore */
  }
  return raw
}

// ---------- Window title ----------

function setWindowTitle(title: string) {
  const fullTitle = `${title} — ${i18n.t('welcome.defaultSubtitle')}`
  document.title = fullTitle
  // titleBarStyle: Overlay 用 HTML <title> 显示，原生 window title 留空避免重复。
  import('@tauri-apps/api/webviewWindow')
    .then(({ getCurrentWebviewWindow }) => {
      getCurrentWebviewWindow().setTitle(' ').catch(() => {})
    })
    .catch(() => {})
}

// ---------- Store ----------

export const useBrandingStore = create<BrandingState>((set) => ({
  ...DEFAULTS,
  isCustom: false,

  applyBranding(tenant) {
    if (!tenant) return

    const productName = hasValue(tenant.productName) ? tenant.productName : DEFAULTS.productName
    const logoUrl = hasValue(tenant.logoUrl) ? resolveLogoUrl(tenant.logoUrl) : DEFAULTS.logoUrl
    const fontFamily = hasValue(tenant.fontFamily) ? tenant.fontFamily : DEFAULTS.fontFamily

    const accentColor = normalizeHex(tenant.accentColor, DEFAULTS.accentColor)
    const primaryColor = normalizeHex(tenant.primaryColor, DEFAULTS.primaryColor)
    const bgColor = normalizeHex(tenant.bgColor, DEFAULTS.bgColor)
    const sidebarBgColor = normalizeHex(tenant.sidebarBgColor, DEFAULTS.sidebarBgColor)

    deriveAccentPalette(accentColor)
    derivePrimaryPalette(primaryColor)
    deriveBackgroundPalette(bgColor, primaryColor)
    deriveSidebarPalette(sidebarBgColor, primaryColor)

    if (hasValue(tenant.fontFamily)) {
      setVar('--font-sans', FONT_MAP[fontFamily] || fontFamily)
    } else {
      removeVar('--font-sans')
    }

    setWindowTitle(productName)

    const isCustom =
      accentColor !== DEFAULTS.accentColor ||
      primaryColor !== DEFAULTS.primaryColor ||
      bgColor !== DEFAULTS.bgColor ||
      sidebarBgColor !== DEFAULTS.sidebarBgColor ||
      productName !== DEFAULTS.productName ||
      logoUrl !== DEFAULTS.logoUrl ||
      hasValue(tenant.fontFamily)

    set({
      productName,
      logoUrl,
      accentColor,
      primaryColor,
      bgColor,
      sidebarBgColor,
      fontFamily,
      isCustom,
    })

    // Mirror to disk so a returning user (or post-logout login page) sees
    // the same brand before they re-enter credentials. Tenant.logoUrl is
    // stored raw — resolveLogoUrl gets re-applied on next load.
    if (isCustom) {
      void saveLastBrand({
        productName: tenant.productName,
        logoUrl: tenant.logoUrl,
        accentColor: tenant.accentColor,
        primaryColor: tenant.primaryColor,
        bgColor: tenant.bgColor,
        sidebarBgColor: tenant.sidebarBgColor,
        fontFamily: tenant.fontFamily,
      }).catch((err) => {
        console.warn('[brandingStore] saveLastBrand failed:', err)
      })
    }
  },

  reset() {
    ALL_OVERRIDDEN_VARS.forEach(removeVar)
    setWindowTitle(DEFAULTS.productName)
    set({ ...DEFAULTS, isCustom: false })
  },

  async restoreFromDisk() {
    try {
      const snap: BrandSnapshot | null = await getLastBrand()
      if (!snap) return
      // Reuse applyBranding so derive + isCustom stay consistent. The snap
      // already lives in TenantBranding shape (camelCase, all-optional).
      useBrandingStore.getState().applyBranding(snap)
    } catch (err) {
      console.warn('[brandingStore] restoreFromDisk failed:', err)
    }
  },
}))
