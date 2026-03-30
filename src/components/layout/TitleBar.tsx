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
import { isDarkColor } from '@/lib/themeUtils'

export function TitleBar() {
  const accentColor = useBrandingStore((s) => s.accentColor)
  const productName = useBrandingStore((s) => s.productName)
  const isCustom = useBrandingStore((s) => s.isCustom)

  const bg = isCustom ? accentColor : 'var(--color-bg-sidebar)'
  const textColor = isCustom ? (isDarkColor(accentColor) ? 'rgba(255,255,255,0.9)' : 'rgba(0,0,0,0.7)') : 'var(--color-text-muted)'

  return (
    <div
      data-tauri-drag-region
      className="flex h-7 shrink-0 items-center justify-center"
      style={{ background: bg, paddingLeft: 78 }}
    >
      <span
        data-tauri-drag-region
        className="text-xs font-medium select-none"
        style={{ color: textColor }}
      >
        {productName}
      </span>
    </div>
  )
}
