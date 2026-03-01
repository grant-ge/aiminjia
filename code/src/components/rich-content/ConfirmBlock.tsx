/**
 * ConfirmBlock — human-in-the-loop confirmation card with primary left border.
 * Based on visual-prototype-zh.html .confirm styles.
 */
import type { ConfirmBlock as ConfirmBlockType } from '@/types/message'
import { Button } from '@/components/common/Button'

interface ConfirmBlockProps {
  confirm: ConfirmBlockType
  onConfirm?: (action: string) => void
  onReject?: (action: string) => void
}

export function ConfirmBlock({ confirm, onConfirm, onReject }: ConfirmBlockProps) {
  const isPending = confirm.status === 'pending'

  return (
    <div
      className="my-3 rounded-lg border border-l-[3px] p-4"
      style={{
        background: isPending
          ? 'var(--color-primary-subtle)'
          : 'var(--color-bg-card)',
        borderColor: isPending
          ? 'var(--color-primary-muted)'
          : 'var(--color-border)',
        borderLeftColor: isPending
          ? 'var(--color-primary)'
          : confirm.status === 'confirmed'
            ? 'var(--color-semantic-green)'
            : 'var(--color-semantic-red)',
      }}
    >
      {/* Title */}
      <div
        className="mb-2 text-base font-semibold"
        style={{ color: 'var(--color-primary)' }}
      >
        {confirm.title}
      </div>

      {/* Action buttons */}
      {isPending ? (
        <div className="flex items-center gap-2">
          <Button
            variant="primary"
            onClick={() => onConfirm?.(confirm.primaryAction)}
          >
            {confirm.primaryLabel}
          </Button>
          {confirm.secondaryLabel && confirm.secondaryAction && (
            <Button
              variant="secondary"
              onClick={() => onReject?.(confirm.secondaryAction!)}
            >
              {confirm.secondaryLabel}
            </Button>
          )}
        </div>
      ) : (
        <div
          className="text-sm font-medium"
          style={{
            color:
              confirm.status === 'confirmed'
                ? 'var(--color-semantic-green)'
                : 'var(--color-semantic-red)',
          }}
        >
          {confirm.status === 'confirmed' ? 'Confirmed' : 'Rejected'}
        </div>
      )}
    </div>
  )
}
