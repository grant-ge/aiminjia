/**
 * @designSource design.pen#giMe2/kFHCj/vHMr4
 * @sizing 980 × auto, r-18, shadow lvl-3; overlay #0000004d
 */
import { X } from 'lucide-react'
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

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
  const { t } = useTranslation()

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
        // spec §7.3 — full modal overlay (settings is a modal, not a drawer)
        className="absolute inset-0 bg-[var(--color-overlay-light)]"
        onClick={onClose}
      />
      <div
        data-aijia-settings-shell
        data-testid="settings-modal-box"
        // spec §8.2 Modal xl 980×720; §5 shadow-modal token
        className="relative z-10 grid h-[720px] w-[980px] max-h-[calc(100vh-48px)] max-w-[calc(100vw-48px)] grid-cols-[220px_minmax(0,1fr)] overflow-hidden rounded-xl border border-border bg-card shadow-[var(--shadow-modal)]"
        style={{ height }}
      >
        {menu}
        {content}
        <button
          type="button"
          aria-label={t('common.close')}
          data-aijia-settings-action="close"
          data-testid="settings-close-button"
          onClick={onClose}
          className="absolute right-3 top-3 z-20 inline-flex h-8 w-8 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <X className="h-4 w-4" />
        </button>
      </div>
    </div>
  )
}
