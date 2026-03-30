/**
 * TitleBar — replaces macOS native title bar content.
 * With titleBarStyle: "overlay", the native title bar is transparent and
 * HTML content extends underneath. This component fills that area with
 * the accent color and provides a drag region.
 *
 * Height: 28px (macOS standard title bar height).
 * Left padding: 78px (space for red/yellow/green traffic light buttons).
 */
import { useBrandingStore } from '@/stores/brandingStore'
import { useProductName } from '@/hooks/useProductName'
import { useTranslation } from 'react-i18next'
import { isDarkColor } from '@/lib/themeUtils'

export function TitleBar() {
  const { t } = useTranslation()
  const accentColor = useBrandingStore((s) => s.accentColor)
  const productName = useProductName()
  const isCustom = useBrandingStore((s) => s.isCustom)

  const bg = isCustom ? accentColor : 'var(--color-bg-sidebar)'
  const textColor = isCustom ? (isDarkColor(accentColor) ? 'rgba(255,255,255,0.9)' : 'rgba(0,0,0,0.7)') : 'var(--color-text-muted)'

  return (
    <div
      data-tauri-drag-region
      className="flex h-7 w-full shrink-0 items-center"
      style={{ background: bg }}
    >
      {/* Space for red/yellow/green traffic light buttons */}
      <div data-tauri-drag-region className="w-[78px] shrink-0" />
      {/* Centered title */}
      <span
        data-tauri-drag-region
        className="flex-1 text-center text-xs font-medium select-none"
        style={{ color: textColor }}
      >
        {productName} — {t('welcome.defaultSubtitle')}
      </span>
      {/* Right balance space */}
      <div data-tauri-drag-region className="w-[78px] shrink-0" />
    </div>
  )
}
