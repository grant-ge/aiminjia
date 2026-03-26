/**
 * brandingStore — runtime brand customization from tenant config.
 *
 * From 3 base colors (accent, primary, bg) + sidebar bg, derives the full
 * set of ~30 CSS variables that globals.css defines. This ensures theme
 * changes are visually obvious across every UI element.
 */
import { create } from 'zustand'

const DEFAULTS = {
  productName: 'AI小家',
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
  logoUrl: string
  accentColor: string
  primaryColor: string
  bgColor: string
  sidebarBgColor: string
  fontFamily: string
  isCustom: boolean

  applyBranding(tenant: { productName?: string; logoUrl?: string; accentColor?: string; primaryColor?: string; bgColor?: string; sidebarBgColor?: string; fontFamily?: string } | null): void
  reset(): void
}

// --- Color utilities ---

function hexToRgb(hex: string): [number, number, number] {
  const r = parseInt(hex.slice(1, 3), 16)
  const g = parseInt(hex.slice(3, 5), 16)
  const b = parseInt(hex.slice(5, 7), 16)
  return [r, g, b]
}

function rgbToHex(r: number, g: number, b: number): string {
  const clamp = (v: number) => Math.max(0, Math.min(255, Math.round(v)))
  return `#${clamp(r).toString(16).padStart(2, '0')}${clamp(g).toString(16).padStart(2, '0')}${clamp(b).toString(16).padStart(2, '0')}`
}

/** Mix color towards white (factor 0-1) */
function lighten(hex: string, factor: number): string {
  const [r, g, b] = hexToRgb(hex)
  return rgbToHex(r + (255 - r) * factor, g + (255 - g) * factor, b + (255 - b) * factor)
}

/** Mix color towards black (factor 0-1) */
function darken(hex: string, factor: number): string {
  const [r, g, b] = hexToRgb(hex)
  return rgbToHex(r * (1 - factor), g * (1 - factor), b * (1 - factor))
}

/** Generate rgba string from hex + alpha */
function rgba(hex: string, alpha: number): string {
  const [r, g, b] = hexToRgb(hex)
  return `rgba(${r}, ${g}, ${b}, ${alpha})`
}

/** Determine if a color is dark (for choosing text-on-color) */
function isDark(hex: string): boolean {
  const [r, g, b] = hexToRgb(hex)
  return (r * 0.299 + g * 0.587 + b * 0.114) < 128
}

/** Mix two hex colors */
function mixColors(hex1: string, hex2: string, weight: number): string {
  const [r1, g1, b1] = hexToRgb(hex1)
  const [r2, g2, b2] = hexToRgb(hex2)
  return rgbToHex(
    r1 * weight + r2 * (1 - weight),
    g1 * weight + g2 * (1 - weight),
    b1 * weight + b2 * (1 - weight),
  )
}

// --- CSS injection ---

function setCSSVar(name: string, value: string) {
  document.documentElement.style.setProperty(name, value)
}

function removeCSSVar(name: string) {
  document.documentElement.style.removeProperty(name)
}

/** Derive full accent palette from a single accent color */
function deriveAccentPalette(accent: string) {
  setCSSVar('--color-accent', accent)
  setCSSVar('--color-accent-hover', darken(accent, 0.1))
  setCSSVar('--color-accent-active', darken(accent, 0.2))
  setCSSVar('--color-accent-light', lighten(accent, 0.4))
  // Full shade scale (50-700)
  setCSSVar('--color-accent-50', lighten(accent, 0.9))
  setCSSVar('--color-accent-100', lighten(accent, 0.75))
  setCSSVar('--color-accent-200', lighten(accent, 0.55))
  setCSSVar('--color-accent-300', lighten(accent, 0.35))
  setCSSVar('--color-accent-400', accent)
  setCSSVar('--color-accent-500', darken(accent, 0.1))
  setCSSVar('--color-accent-600', darken(accent, 0.2))
  setCSSVar('--color-accent-700', darken(accent, 0.35))
  // Alpha variants
  setCSSVar('--color-accent-subtle', rgba(accent, 0.12))
  setCSSVar('--color-accent-muted', rgba(accent, 0.25))
  setCSSVar('--color-accent-bg-light', rgba(accent, 0.04))
  setCSSVar('--color-accent-border', rgba(accent, 0.25))
  // Text on accent
  setCSSVar('--color-text-on-accent', isDark(accent) ? '#FFFFFF' : '#1A1A1A')
}

/** Derive full primary palette from a single primary color */
function derivePrimaryPalette(primary: string) {
  setCSSVar('--color-primary', primary)
  setCSSVar('--color-primary-hover', lighten(primary, 0.15))
  setCSSVar('--color-primary-active', darken(primary, 0.1))
  setCSSVar('--color-text-primary', primary)
  setCSSVar('--color-text-on-primary', isDark(primary) ? '#FFFFFF' : '#1A1A1A')
  // Alpha variants
  setCSSVar('--color-primary-subtle', rgba(primary, 0.08))
  setCSSVar('--color-primary-muted', rgba(primary, 0.15))
  // Derived text shades
  setCSSVar('--color-text-secondary', lighten(primary, 0.3))
  setCSSVar('--color-text-muted', lighten(primary, 0.5))
  setCSSVar('--color-text-disabled', lighten(primary, 0.7))
}

/** Derive background palette from main bg + sidebar bg */
function deriveBgPalette(bg: string, sidebarBg: string) {
  setCSSVar('--color-bg-main', bg)
  setCSSVar('--color-bg-sidebar', sidebarBg)
  setCSSVar('--color-bg-sidebar-hover', darken(sidebarBg, 0.04))

  // Determine if this is a dark theme
  const dark = isDark(bg)

  if (dark) {
    // Dark theme: elevated surfaces are lighter than base
    setCSSVar('--color-bg-base', darken(bg, 0.1))
    setCSSVar('--color-bg-elevated', lighten(bg, 0.08))
    setCSSVar('--color-bg-card', lighten(bg, 0.06))
    setCSSVar('--color-bg-card-hover', lighten(bg, 0.1))
    setCSSVar('--color-bg-input', lighten(bg, 0.08))
    setCSSVar('--color-bg-msg-user', lighten(bg, 0.1))
    setCSSVar('--color-bg-code', lighten(bg, 0.06))
    setCSSVar('--color-bg-code-header', rgba('#FFFFFF', 0.04))
    setCSSVar('--color-bg-neutral', rgba('#AAAAAA', 0.15))
    setCSSVar('--color-bg-neutral-subtle', rgba('#AAAAAA', 0.1))
    // Dark borders
    setCSSVar('--color-border', rgba('#FFFFFF', 0.12))
    setCSSVar('--color-border-light', rgba('#FFFFFF', 0.08))
    setCSSVar('--color-border-subtle', rgba('#FFFFFF', 0.06))
    // Code text
    setCSSVar('--color-text-code', '#E0E0E0')
  } else {
    // Light theme: derive from bg
    setCSSVar('--color-bg-base', darken(bg, 0.03))
    setCSSVar('--color-bg-elevated', '#FFFFFF')
    setCSSVar('--color-bg-card', '#FFFFFF')
    setCSSVar('--color-bg-card-hover', mixColors(bg, sidebarBg, 0.5))
    setCSSVar('--color-bg-input', '#FFFFFF')
    setCSSVar('--color-bg-msg-user', darken(bg, 0.03))
    setCSSVar('--color-bg-code', darken(bg, 0.02))
    setCSSVar('--color-bg-code-header', rgba('#000000', 0.02))
    setCSSVar('--color-bg-neutral', rgba('#A8A8A8', 0.12))
    setCSSVar('--color-bg-neutral-subtle', rgba('#A8A8A8', 0.1))
    // Light borders
    const borderBase = darken(bg, 0.1)
    setCSSVar('--color-border', borderBase)
    setCSSVar('--color-border-light', darken(bg, 0.15))
    setCSSVar('--color-border-subtle', darken(bg, 0.05))
    // Code text
    setCSSVar('--color-text-code', '#383838')
  }
}

// --- All CSS vars that we override (for reset) ---
const ALL_CSS_VARS = [
  // Accent
  '--color-accent', '--color-accent-hover', '--color-accent-active', '--color-accent-light',
  '--color-accent-50', '--color-accent-100', '--color-accent-200', '--color-accent-300',
  '--color-accent-400', '--color-accent-500', '--color-accent-600', '--color-accent-700',
  '--color-accent-subtle', '--color-accent-muted', '--color-accent-bg-light', '--color-accent-border',
  '--color-text-on-accent',
  // Primary
  '--color-primary', '--color-primary-hover', '--color-primary-active',
  '--color-text-primary', '--color-text-on-primary',
  '--color-primary-subtle', '--color-primary-muted',
  '--color-text-secondary', '--color-text-muted', '--color-text-disabled',
  // Background
  '--color-bg-main', '--color-bg-sidebar', '--color-bg-sidebar-hover',
  '--color-bg-base', '--color-bg-elevated', '--color-bg-card', '--color-bg-card-hover',
  '--color-bg-input', '--color-bg-msg-user', '--color-bg-code', '--color-bg-code-header',
  '--color-bg-neutral', '--color-bg-neutral-subtle',
  // Border
  '--color-border', '--color-border-light', '--color-border-subtle',
  // Code
  '--color-text-code',
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
  const fullTitle = `${title} — 智能工作助手`
  document.title = fullTitle
  import('@tauri-apps/api/webviewWindow').then(({ getCurrentWebviewWindow }) => {
    getCurrentWebviewWindow().setTitle(fullTitle).catch(() => {})
  }).catch(() => {})
}

export const useBrandingStore = create<BrandingState>((set) => ({
  ...DEFAULTS,
  isCustom: false,

  applyBranding(tenant) {
    if (!tenant) return

    const productName = hasValue(tenant.productName) ? tenant.productName : DEFAULTS.productName
    const logoUrl = hasValue(tenant.logoUrl) ? resolveLogoUrl(tenant.logoUrl) : DEFAULTS.logoUrl
    const accentColor = hasValue(tenant.accentColor) ? tenant.accentColor : DEFAULTS.accentColor
    const primaryColor = hasValue(tenant.primaryColor) ? tenant.primaryColor : DEFAULTS.primaryColor
    const bgColor = hasValue(tenant.bgColor) ? tenant.bgColor : DEFAULTS.bgColor
    const sidebarBgColor = hasValue(tenant.sidebarBgColor) ? tenant.sidebarBgColor : DEFAULTS.sidebarBgColor
    const fontFamily = hasValue(tenant.fontFamily) ? tenant.fontFamily : DEFAULTS.fontFamily

    const isCustom = hasValue(tenant.productName) || hasValue(tenant.logoUrl)
      || hasValue(tenant.accentColor) || hasValue(tenant.primaryColor)
      || hasValue(tenant.bgColor) || hasValue(tenant.sidebarBgColor)
      || hasValue(tenant.fontFamily)

    // Derive full palettes from base colors
    if (isCustom) {
      deriveAccentPalette(accentColor)
      derivePrimaryPalette(primaryColor)
      deriveBgPalette(bgColor, sidebarBgColor)
      if (hasValue(tenant.fontFamily)) {
        setCSSVar('--font-sans', FONT_MAP[fontFamily] || FONT_MAP[''])
      }
    }

    setWindowTitle(productName)
    set({ productName, logoUrl, accentColor, primaryColor, bgColor, sidebarBgColor, fontFamily, isCustom })
  },

  reset() {
    ALL_CSS_VARS.forEach(removeCSSVar)
    setWindowTitle(DEFAULTS.productName)
    set({ ...DEFAULTS, isCustom: false })
  },
}))
