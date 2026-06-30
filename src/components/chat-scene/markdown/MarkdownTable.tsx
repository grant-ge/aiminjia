import { useCallback, useState } from 'react'
import { Check, Copy } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { toCsv, toTsv } from '@/components/data-table/tableUtils'
import type { TableColumn, TableRow } from '@/components/data-table'
import { extractTableFromGfm } from './extractTableFromGfm'
import { Button } from '@/components/ui/button'

interface MarkdownTableProps {
  children?: React.ReactNode
}

export function MarkdownTable({ children }: MarkdownTableProps) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState<'idle' | 'ok' | 'fail'>('idle')
  const { columns, rows } = extractTableFromGfm(children)

  const handleCopy = useCallback(
    (e: React.MouseEvent) => {
      const text = e.shiftKey
        ? toTsv(columns as TableColumn[], rows as TableRow[])
        : toCsv(columns as TableColumn[], rows as TableRow[])
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

  return (
    <div className="markdown-table-wrap">
      <div className="markdown-table-scroll">
        <table>{children}</table>
      </div>
      <div className="markdown-table-actions">
        <Button
          type="button"
          link
          className="markdown-table-copy gap-1 text-[var(--color-text-muted)]"
          onClick={handleCopy}
          title={t('dataTable.copyCsv', 'Copy as CSV (hold Shift for TSV)')}
          data-testid="markdown-table-copy-button"
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
      </div>
    </div>
  )
}
