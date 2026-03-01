/**
 * Modal — overlay + centered card with size variants and entrance animation.
 * Based on visual-prototype-zh.html .modal styles.
 */
import type { ReactNode } from 'react'

type ModalSize = 'sm' | 'md' | 'lg'

const WIDTH_MAP: Record<ModalSize, string> = {
  sm: '400px',
  md: '520px',
  lg: '640px',
}

interface ModalProps {
  open: boolean
  onClose: () => void
  title: string
  children: ReactNode
  footer?: ReactNode
  size?: ModalSize
}

export function Modal({ open, onClose, title, children, footer, size = 'md' }: ModalProps) {
  if (!open) return null

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center"
      style={{ background: 'var(--color-overlay)' }}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div
        className="max-h-[80vh] overflow-y-auto rounded-lg border animate-[modalIn_0.2s_ease-out]"
        style={{
          width: WIDTH_MAP[size],
          background: 'var(--color-bg-card)',
          borderColor: 'var(--color-border)',
          boxShadow: 'var(--shadow-modal)',
        }}
      >
        {/* Header */}
        <div
          className="flex items-center justify-between border-b px-5 py-3.5"
          style={{ borderColor: 'var(--color-border)' }}
        >
          <h3 className="text-lg font-semibold">{title}</h3>
          <button
            className="cursor-pointer border-none bg-transparent p-1 text-lg leading-none transition-colors duration-150"
            style={{ color: 'var(--color-text-muted)' }}
            onClick={onClose}
            onMouseEnter={(e) => {
              e.currentTarget.style.color = 'var(--color-text-secondary)'
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.color = 'var(--color-text-muted)'
            }}
          >
            &times;
          </button>
        </div>

        {/* Body */}
        <div className="p-5">{children}</div>

        {/* Footer */}
        {footer && (
          <div
            className="flex items-center justify-end gap-2 border-t px-5 py-3"
            style={{ borderColor: 'var(--color-border)' }}
          >
            {footer}
          </div>
        )}
      </div>
    </div>
  )
}
