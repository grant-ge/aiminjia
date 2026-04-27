import React from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useBrandingStore } from '@/stores/brandingStore'
import { useProductName } from '@/hooks/useProductName'
import { useTranslation } from 'react-i18next'
import { isDarkColor } from '@/lib/themeUtils'

const isWindows = navigator.userAgent.includes('Windows')

/**
 * Double-click to maximize; single-click to start drag.
 * Belt-and-suspenders: data-tauri-drag-region handles native drag (incl. touch/pen),
 * this handler adds double-click-to-maximize which data-tauri-drag-region doesn't support.
 */
function handleDragStart(e: React.MouseEvent) {
  if (e.buttons === 1 && e.detail === 2) {
    getCurrentWindow().toggleMaximize()
  }
  // Single-click drag is handled natively by data-tauri-drag-region.
  // Fallback for environments where the attribute isn't honored:
  if (e.buttons === 1 && e.detail === 1) {
    getCurrentWindow().startDragging()
  }
}

/** Windows custom window control buttons (minimize / maximize / close). */
function WindowControls({ color }: { color: string }) {
  const win = getCurrentWindow()
  const btnClass = 'flex h-7 w-11 items-center justify-center transition-colors'
  return (
    <div className="flex shrink-0">
      <button className={btnClass} style={{ color }} onClick={() => win.minimize()}
        aria-label="Minimize">
        <svg width="10" height="1" viewBox="0 0 10 1"><rect fill="currentColor" width="10" height="1"/></svg>
      </button>
      <button className={btnClass} style={{ color }} onClick={() => win.toggleMaximize()}
        aria-label="Maximize">
        <svg width="10" height="10" viewBox="0 0 10 10"><rect fill="none" stroke="currentColor" strokeWidth="1" x="0.5" y="0.5" width="9" height="9"/></svg>
      </button>
      <button className={`${btnClass} hover:bg-red-600 hover:text-white`} style={{ color }}
        onClick={() => win.close()} aria-label="Close">
        <svg width="10" height="10" viewBox="0 0 10 10"><path fill="currentColor" d="M1.7.3.3 1.7 3.6 5 .3 8.3l1.4 1.4L5 6.4l3.3 3.3 1.4-1.4L6.4 5l3.3-3.3L8.3.3 5 3.6 1.7.3z"/></svg>
      </button>
    </div>
  )
}

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
      onMouseDown={handleDragStart}
    >
      {/* macOS: space for traffic light buttons (no drag here so OS receives clicks). */}
      {!isWindows && <div className="w-[78px] shrink-0" />}
      {/* Centered title — pointer-events-none so drag passes through */}
      <span
        data-tauri-drag-region
        className="flex-1 text-center text-xs font-medium select-none pointer-events-none"
        style={{ color: textColor }}
      >
        {productName} — {t('welcome.defaultSubtitle')}
      </span>
      {/* macOS: right spacer for symmetry. Windows: custom window controls. */}
      {isWindows ? <WindowControls color={textColor} /> : <div className="w-[78px] shrink-0" />}
    </div>
  )
}
