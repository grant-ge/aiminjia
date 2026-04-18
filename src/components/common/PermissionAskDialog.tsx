import { useEffect } from 'react'

import { Button } from './Button'
import { Modal } from './Modal'
import type { PendingAsk } from '@/stores/streamingStore'

interface PermissionAskDialogProps {
  open: boolean
  ask: PendingAsk | null
  onAllow: () => void
  onDeny: () => void
  onCancel: () => void
}

export function PermissionAskDialog({
  open,
  ask,
  onAllow,
  onDeny,
  onCancel,
}: PermissionAskDialogProps) {
  useEffect(() => {
    if (!open) return

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        onCancel()
      }
    }

    document.addEventListener('keydown', handleKeyDown)
    return () => document.removeEventListener('keydown', handleKeyDown)
  }, [open, onCancel])

  if (!open || !ask) {
    return null
  }

  return (
    <Modal
      open={open}
      onClose={onCancel}
      title="工具执行请求"
      size="sm"
      footer={(
        <>
          <Button variant="secondary" onClick={onDeny}>
            拒绝
          </Button>
          <Button variant="primary" onClick={onAllow}>
            允许
          </Button>
        </>
      )}
    >
      <div className="flex flex-col gap-3">
        <div
          className="text-sm font-semibold"
          style={{ color: 'var(--color-text-primary)' }}
        >
          {ask.toolName}
        </div>

        <p
          className="text-sm leading-relaxed"
          style={{ color: 'var(--color-text-secondary)' }}
        >
          {ask.message}
        </p>

        {ask.suggestions && ask.suggestions.length > 0 && (
          <div className="flex flex-wrap gap-1.5">
            {ask.suggestions.map((suggestion) => (
              <span
                key={suggestion}
                className="rounded px-2 py-0.5 text-xs"
                style={{
                  background: 'var(--color-bg-subtle)',
                  color: 'var(--color-text-muted)',
                  border: '1px solid var(--color-border)',
                }}
              >
                {suggestion}
              </span>
            ))}
          </div>
        )}
      </div>
    </Modal>
  )
}
