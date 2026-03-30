import i18n from '@/i18n'
/**
 * brandingStore — runtime brand customization from tenant config.
 *
 * Colors are set by tenant admin via lotus portal.
 * Desktop client receives colors on login and applies via CSS custom properties.
 *
 * Architecture:
 * - globals.css :root {} defines default color values
 * - applyBranding() overrides them via document.documentElement.style.setProperty()
 * - reset() clears overrides via removeProperty(), falling back to :root defaults
 */
import { create } from 'zustand'
import { lighten, darken, rgba, isDarkColor } from '@/lib/themeUtils'

export const DEFAULTS = {
  productName: 'AI小家',
  productNameEn: 'AIjia',
  logoUrl: '/app-icon.png',
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

  applyBranding(tenant: {
    productName?: string
    logoUrl?: string
    accentColor?: string
    primaryColor?: string
    bgColor?: string
    sidebarBgColor?: string
    fontFamily?: string
  } | null): void
  reset(): void
}

// --- CSS variable helpers (inline style on :root, guaranteed to override :root {} rules) ---

function setVar(name: string, value: string) {
  document.documentElement.style.setProperty(name, value)
}

function removeVar(name: string) {
  document.documentElement.style.removeProperty(name)
}

// --- Palette derivation ---

function deriveAccentPalette(accent: string) {
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
  setVar('--color-text-on-accent', isDarkColor(accent) ? '#FFFFFF' : '#1A1A1A')
}

function derivePrimaryPalette(primary: string) {
  const dark = isDarkColor(primary)
  setVar('--color-primary', primary)
  setVar('--color-primary-hover', dark ? lighten(primary, 0.15) : darken(primary, 0.1))
  setVar('--color-primary-active', dark ? lighten(primary, 0.25) : darken(primary, 0.2))
  setVar('--color-text-on-primary', dark ? '#FFFFFF' : '#1A1A1A')
  setVar('--color-primary-subtle', rgba(primary, 0.08))
  setVar('--color-primary-muted', rgba(primary, 0.15))
}

// All CSS vars that get overridden (for reset)
const ALL_CSS_VARS = [
  // Accent
  '--color-accent', '--color-accent-hover', '--color-accent-active', '--color-accent-light',
  '--color-accent-50', '--color-accent-100', '--color-accent-200', '--color-accent-300',
  '--color-accent-400', '--color-accent-500', '--color-accent-600', '--color-accent-700',
  '--color-accent-subtle', '--color-accent-muted', '--color-accent-bg-light', '--color-accent-border',
  '--color-text-on-accent',
  // Primary
  '--color-primary', '--color-primary-hover', '--color-primary-active',
  '--color-text-on-primary', '--color-primary-subtle', '--color-primary-muted',
  // Background (subtle tint)
  '--color-bg-main', '--color-bg-sidebar',
  // Font
  '--font-sans',
]

function hasValue(s?: string | null): s is string {
  return !!s && s.trim().length > 0
}

function resolveLogoUrl(raw: string): string {
  if (!raw || raw.startsWith('/')) return raw
  try {
    const u = new URL(raw)
    if (u.protocol === 'http:' || u.protocol === 'https:') {
      return `https://ai-tenant.renlijia.com/api/file?url=${encodeURIComponent(raw)}`
    }
  } catch { /* ignore */ }
  return raw
}

function setWindowTitle(title: string) {
  const fullTitle = `${title} — ${i18n.t('welcome.defaultSubtitle')}`
  document.title = fullTitle
  // Set window title to empty string to avoid duplicate text in overlay titlebar
  import('@tauri-apps/api/webviewWindow').then(({ getCurrentWebviewWindow }) => {
    getCurrentWebviewWindow().setTitle(' ').catch(() => {})
  }).catch(() => {})
}

export const useBrandingStore = create<BrandingState>((set) => ({
  ...DEFAULTS,
  isCustom: false,

  applyBranding(tenant) {
    if (!tenant) return

    const productName = hasValue(tenant.productName) ? tenant.productName : DEFAULTS.productName
    const logoUrl = hasValue(tenant.logoUrl) ? resolveLogoUrl(tenant.logoUrl) : DEFAULTS.logoUrl
    const fontFamily = hasValue(tenant.fontFamily) ? tenant.fontFamily : DEFAULTS.fontFamily
    const accentColor = hasValue(tenant.accentColor) ? tenant.accentColor : DEFAULTS.accentColor
    const primaryColor = hasValue(tenant.primaryColor) ? tenant.primaryColor : DEFAULTS.primaryColor
    const bgColor = hasValue(tenant.bgColor) ? tenant.bgColor : DEFAULTS.bgColor
    const sidebarBgColor = hasValue(tenant.sidebarBgColor) ? tenant.sidebarBgColor : DEFAULTS.sidebarBgColor

    const isCustom = hasValue(tenant.accentColor) || hasValue(tenant.primaryColor)

    if (isCustom) {
      deriveAccentPalette(accentColor)
      derivePrimaryPalette(primaryColor)
      // Subtle bg tint if tenant provided bg/sidebarBg colors
      if (hasValue(tenant.bgColor)) {
        setVar('--color-bg-main', bgColor)
      }
      if (hasValue(tenant.sidebarBgColor)) {
        setVar('--color-bg-sidebar', sidebarBgColor)
      }
    } else {
      ALL_CSS_VARS.forEach(removeVar)
    }

    if (hasValue(tenant.fontFamily)) {
      setVar('--font-sans', FONT_MAP[fontFamily] || FONT_MAP[''])
    } else {
      removeVar('--font-sans')
    }

    setWindowTitle(productName)
    set({ productName, logoUrl, accentColor, primaryColor, bgColor, sidebarBgColor, fontFamily, isCustom })
  },

  reset() {
    ALL_CSS_VARS.forEach(removeVar)
    setWindowTitle(DEFAULTS.productName)
    set({ ...DEFAULTS, isCustom: false })
  },
}))
