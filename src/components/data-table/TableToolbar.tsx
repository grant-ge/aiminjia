import { useState, useCallback, useEffect } from 'react'
import { Check, Copy } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { TableColumn, TableRow } from './tableSchema'
import { toCsv, toTsv } from './tableUtils'
import { Button } from '@/components/ui/button'

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

  return (
    <Button
      type="button"
      link
      onClick={handleCopy}
      title={tooltip}
      className="gap-1 text-[var(--color-text-muted)]"
      data-testid="table-copy-button"
      icon={copied === 'ok'
        ? <Check className="h-3.5 w-3.5" aria-hidden="true" />
        : <Copy className="h-3.5 w-3.5" aria-hidden="true" />
      }
    >
      <span>
        {copied === 'ok'
          ? t('common.copied', 'Copied')
          : copied === 'fail'
            ? t('common.copyFailed', 'Copy failed')
            : t('common.copy', 'Copy')}
      </span>
    </Button>
  )
}
