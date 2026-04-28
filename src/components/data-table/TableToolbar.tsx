import { useState, useCallback, useEffect } from 'react'
import { Copy } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { TableColumn, TableRow } from './tableSchema'
import { toCsv, toTsv } from './tableUtils'

interface Props {
  enableCopy?: boolean
  columns: TableColumn[]
  rows: TableRow[]
}

export function TableToolbar({ enableCopy, columns, rows }: Props) {
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

  if (!enableCopy) return null

  const tooltip = shiftHeld
    ? t('dataTable.copyTsv', 'Copy as TSV')
    : t('dataTable.copyCsv', 'Copy as CSV (hold Shift for TSV)')
  const toneClass =
    copied === 'ok'
      ? 'text-[var(--color-semantic-green)]'
      : copied === 'fail'
        ? 'text-[var(--color-semantic-red)]'
        : 'text-[var(--color-text-muted)] hover:text-[var(--primary)]'

  return (
    <button
      type="button"
      onClick={handleCopy}
      title={tooltip}
      className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[0.9375rem] transition-colors ${toneClass}`}
      style={{ background: 'transparent' }}
      data-testid="table-copy-button"
    >
      <Copy size={15} strokeWidth={2} aria-hidden="true" />
      <span>
        {copied === 'ok'
          ? t('common.copied', 'Copied')
          : copied === 'fail'
            ? t('common.copyFailed', 'Copy failed')
            : t('common.copy', 'Copy')}
      </span>
    </button>
  )
}
