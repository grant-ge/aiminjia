import { useState, useCallback, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import type { TableColumn, TableRow, TableMeta } from './tableSchema'
import { toCsv, toTsv } from './tableUtils'

interface Props {
  meta?: TableMeta
  enableCopy?: boolean
  columns: TableColumn[]
  rows: TableRow[]
}

export function TableToolbar({ meta, enableCopy, columns, rows }: Props) {
  const { t } = useTranslation()
  const [shiftHeld, setShiftHeld] = useState(false)
  const [copied, setCopied] = useState<'idle' | 'ok' | 'fail'>('idle')

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => setShiftHeld(e.shiftKey)
    window.addEventListener('keydown', onKey)
    window.addEventListener('keyup', onKey)
    return () => {
      window.removeEventListener('keydown', onKey)
      window.removeEventListener('keyup', onKey)
    }
  }, [])

  const handleCopy = useCallback(
    (e: React.MouseEvent) => {
      const useTsv = e.shiftKey
      const text = useTsv ? toTsv(columns, rows) : toCsv(columns, rows)
      navigator.clipboard
        .writeText(text)
        .then(() => {
          setCopied('ok')
          setTimeout(() => setCopied('idle'), 2000)
        })
        .catch(() => {
          setCopied('fail')
          setTimeout(() => setCopied('idle'), 2000)
        })
    },
    [columns, rows],
  )

  if (!meta?.title && !meta?.badge && !enableCopy) return null

  const tooltip = shiftHeld
    ? t('dataTable.copyTsv', 'Copy as TSV')
    : t('dataTable.copyCsv', 'Copy as CSV (hold Shift for TSV)')

  return (
    <div
      className="flex items-center justify-between border-b px-3 py-2"
      style={{
        background: 'var(--table-header-bg)',
        borderColor: 'var(--table-divider)',
        fontSize: 'var(--table-font-size)',
      }}
      data-testid="table-toolbar"
    >
      <div className="flex items-center gap-2 min-w-0">
        {meta?.title && (
          <span
            className="truncate font-semibold"
            style={{ color: 'var(--color-text-primary)' }}
          >
            {meta.title}
          </span>
        )}
        {meta?.badge && (
          <span
            className="inline-block rounded-full px-2 py-0.5 text-xs font-medium"
            style={{
              background: 'var(--table-tone-neutral-bg)',
              color: 'var(--table-tone-neutral-fg)',
            }}
          >
            {meta.badge}
          </span>
        )}
      </div>
      {enableCopy && (
        <button
          type="button"
          onClick={handleCopy}
          title={tooltip}
          className="text-xs transition-colors"
          style={{
            color:
              copied === 'ok'
                ? 'var(--color-semantic-green)'
                : copied === 'fail'
                  ? 'var(--color-semantic-red)'
                  : 'var(--color-text-muted)',
          }}
          data-testid="table-copy-button"
        >
          {copied === 'ok'
            ? t('common.copied', 'Copied')
            : copied === 'fail'
              ? t('common.copyFailed', 'Copy failed')
              : t('common.copy', 'Copy')}
        </button>
      )}
    </div>
  )
}
