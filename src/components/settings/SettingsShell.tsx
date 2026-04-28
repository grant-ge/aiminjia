/**
 * @designSource design.pen#giMe2/kFHCj/vHMr4
 * @sizing 980 × auto, r-18, shadow lvl-3; overlay #0000004d
 */
import { useEffect } from 'react'

import type { ReactNode } from 'react'

interface SettingsShellProps {
  open: boolean
  menu: ReactNode
  content: ReactNode
  onClose: () => void
  height?: number
}

export function SettingsShell({
  open,
  menu,
  content,
  onClose,
  height = 680,
}: SettingsShellProps) {
  useEffect(() => {
    if (!open) return

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        onClose()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [open, onClose])

  if (!open) return null

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-6">
      <div
        data-testid="settings-overlay"
        className="absolute inset-0 bg-gray-950/35"
        onClick={onClose}
      />
      <div
        data-testid="settings-modal-box"
        className="relative z-10 grid h-[720px] w-[980px] max-h-[calc(100vh-48px)] max-w-[calc(100vw-48px)] grid-cols-[220px_minmax(0,1fr)] overflow-hidden rounded-[18px] border border-border bg-card"
        style={{
          height,
          boxShadow: '0 20px 20px rgba(0,0,0,0.10), 0 10px 10px rgba(0,0,0,0.04)',
        }}
      >
        {menu}
        {content}
      </div>
    </div>
  )
}
