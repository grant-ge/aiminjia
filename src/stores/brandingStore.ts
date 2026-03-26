/**
 * brandingStore — runtime brand customization from tenant config.
 *
 * When the user logs into a cloud tenant that has custom branding,
 * this store overrides the default "AI小家" product name, logo,
 * and theme colors. On logout, everything reverts to defaults.
 */
import { create } from 'zustand'

const DEFAULTS = {
  productName: 'AI小家',
  logoUrl: '/renlijia.png',
  accentColor: '#D4A843',
  primaryColor: '#1D1D1F',
}

interface BrandingState {
  productName: string
  logoUrl: string
  accentColor: string
  primaryColor: string
  isCustom: boolean

  /** Apply branding from tenant info (called after login). */
  applyBranding(tenant: { productName?: string; logoUrl?: string; accentColor?: string; primaryColor?: string } | null): void
  /** Reset to defaults (called on logout). */
  reset(): void
}

/** Inject a CSS custom property on :root. */
function setCSSVar(name: string, value: string) {
  document.documentElement.style.setProperty(name, value)
}

/** Remove a CSS custom property override from :root. */
function removeCSSVar(name: string) {
  document.documentElement.style.removeProperty(name)
}

/** Given a hex color, derive lighter/darker variants for the accent palette. */
function deriveAccentPalette(hex: string) {
  setCSSVar('--color-accent', hex)
  // Simple luminance adjustments — darken/lighten by mixing with black/white
  setCSSVar('--color-accent-hover', adjustBrightness(hex, -0.1))
  setCSSVar('--color-accent-active', adjustBrightness(hex, -0.2))
}

function derivePrimaryPalette(hex: string) {
  setCSSVar('--color-primary', hex)
  setCSSVar('--color-primary-hover', adjustBrightness(hex, 0.15))
  setCSSVar('--color-primary-active', adjustBrightness(hex, -0.1))
  setCSSVar('--color-text-primary', hex)
}

/** Adjust hex color brightness by a factor (-1 to 1). Negative = darker. */
function adjustBrightness(hex: string, factor: number): string {
  const r = parseInt(hex.slice(1, 3), 16)
  const g = parseInt(hex.slice(3, 5), 16)
  const b = parseInt(hex.slice(5, 7), 16)

  const adjust = (c: number) => {
    if (factor > 0) {
      return Math.round(c + (255 - c) * factor)
    }
    return Math.round(c * (1 + factor))
  }

  const clamp = (v: number) => Math.max(0, Math.min(255, v))
  const rr = clamp(adjust(r)).toString(16).padStart(2, '0')
  const gg = clamp(adjust(g)).toString(16).padStart(2, '0')
  const bb = clamp(adjust(b)).toString(16).padStart(2, '0')
  return `#${rr}${gg}${bb}`
}

/** Check if a string is a non-empty value (not null, undefined, or blank). */
function hasValue(s?: string | null): s is string {
  return !!s && s.trim().length > 0
}

export const useBrandingStore = create<BrandingState>((set) => ({
  ...DEFAULTS,
  isCustom: false,

  applyBranding(tenant) {
    if (!tenant) return

    const productName = hasValue(tenant.productName) ? tenant.productName : DEFAULTS.productName
    const logoUrl = hasValue(tenant.logoUrl) ? tenant.logoUrl : DEFAULTS.logoUrl
    const accentColor = hasValue(tenant.accentColor) ? tenant.accentColor : DEFAULTS.accentColor
    const primaryColor = hasValue(tenant.primaryColor) ? tenant.primaryColor : DEFAULTS.primaryColor

    const isCustom = hasValue(tenant.productName) || hasValue(tenant.logoUrl)
      || hasValue(tenant.accentColor) || hasValue(tenant.primaryColor)

    // Apply CSS overrides
    if (hasValue(tenant.accentColor)) deriveAccentPalette(accentColor)
    if (hasValue(tenant.primaryColor)) derivePrimaryPalette(primaryColor)

    // Update window title
    document.title = productName

    set({ productName, logoUrl, accentColor, primaryColor, isCustom })
  },

  reset() {
    // Remove CSS overrides
    const vars = [
      '--color-accent', '--color-accent-hover', '--color-accent-active',
      '--color-primary', '--color-primary-hover', '--color-primary-active',
      '--color-text-primary',
    ]
    vars.forEach(removeCSSVar)

    document.title = DEFAULTS.productName

    set({ ...DEFAULTS, isCustom: false })
  },
}))
