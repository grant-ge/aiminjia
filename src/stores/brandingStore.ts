import i18n from '@/i18n'
import { create } from 'zustand'

import { DEFAULT_ACCENT_COLOR, DERIVED_SKIN_KEYS, deriveSkin } from '@/styles/skin'

export const DEFAULTS = {
  productName: 'AI小家',
  productNameEn: 'AIjia',
  logoUrl: '/app-icon.png',
  accentColor: DEFAULT_ACCENT_COLOR,
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
  fontFamily?: string
  // Deprecated: kept for API payload compatibility, ignored by accent-only skin.
  primaryColor?: string
  bgColor?: string
  sidebarBgColor?: string
}

interface BrandingState {
  productName: string
  productNameEn: string
  logoUrl: string
  accentColor: string
  fontFamily: string
  isCustom: boolean

  applyBranding(tenant: TenantBranding | null): void
  reset(): void
}

function setVar(name: string, value: string) {
  document.documentElement.style.setProperty(name, value)
}

function removeVar(name: string) {
  document.documentElement.style.removeProperty(name)
}

function hasValue(s?: string | null): s is string {
  return !!s && s.trim().length > 0
}

function normalizeAccentColor(input?: string): string {
  if (!hasValue(input)) return DEFAULTS.accentColor
  return /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(input) ? input : DEFAULTS.accentColor
}

function resolveLogoUrl(raw: string): string {
  if (!raw || raw.startsWith('/')) return raw
  try {
    const u = new URL(raw)
    if (u.protocol === 'http:' || u.protocol === 'https:') {
      return `https://ai-tenant.renlijia.com/api/file?url=${encodeURIComponent(raw)}`
    }
  } catch {
    // ignore invalid urls and keep raw string
  }
  return raw
}

function setWindowTitle(title: string) {
  const fullTitle = `${title} — ${i18n.t('welcome.defaultSubtitle')}`
  document.title = fullTitle
  // Set window title to empty string to avoid duplicate text in overlay titlebar
  import('@tauri-apps/api/webviewWindow')
    .then(({ getCurrentWebviewWindow }) => {
      getCurrentWebviewWindow().setTitle(' ').catch(() => {})
    })
    .catch(() => {})
}

export const useBrandingStore = create<BrandingState>((set) => ({
  ...DEFAULTS,
  isCustom: false,

  applyBranding(tenant) {
    if (!tenant) return

    const productName = hasValue(tenant.productName) ? tenant.productName : DEFAULTS.productName
    const logoUrl = hasValue(tenant.logoUrl) ? resolveLogoUrl(tenant.logoUrl) : DEFAULTS.logoUrl
    const fontFamily = hasValue(tenant.fontFamily) ? tenant.fontFamily : DEFAULTS.fontFamily
    const accentColor = normalizeAccentColor(tenant.accentColor)

    const skin = deriveSkin(accentColor)
    for (const [key, value] of Object.entries(skin)) {
      setVar(key, value)
    }

    if (hasValue(tenant.fontFamily)) {
      setVar('--font-sans', FONT_MAP[fontFamily] || FONT_MAP[''])
    } else {
      removeVar('--font-sans')
    }

    setWindowTitle(productName)
    set({
      productName,
      logoUrl,
      accentColor,
      fontFamily,
      isCustom: accentColor !== DEFAULTS.accentColor,
    })
  },

  reset() {
    DERIVED_SKIN_KEYS.forEach(removeVar)
    removeVar('--font-sans')
    setWindowTitle(DEFAULTS.productName)
    set({ ...DEFAULTS, isCustom: false })
  },
}))
