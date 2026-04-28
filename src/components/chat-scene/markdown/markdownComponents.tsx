import { Children, isValidElement, type ReactElement, type ReactNode } from 'react'
import type { Components } from 'react-markdown'
import { useTranslation } from 'react-i18next'
import { TableView } from '@/components/data-table'
import type { TableColumn, TableRow } from '@/components/data-table'
import { useNotificationStore } from '@/stores/notificationStore'
import { openFileByName } from '@/lib/tauri'
import { MarkdownCodeBlock } from './MarkdownCodeBlock'

/** Walk react-markdown's <table> children and extract columns + rows. */
function extractTableFromGfm(node: ReactNode): { columns: TableColumn[]; rows: TableRow[] } {
  const columns: TableColumn[] = []
  const rows: TableRow[] = []

  const collectText = (n: ReactNode): string => {
    if (n == null || typeof n === 'boolean') return ''
    if (typeof n === 'string' || typeof n === 'number') return String(n)
    if (Array.isArray(n)) return n.map(collectText).join('')
    if (isValidElement(n)) {
      const props = n.props as { children?: ReactNode }
      return collectText(props.children)
    }
    return ''
  }

  // react-markdown's table children: [<thead>, <tbody>] (whitespace text nodes are skipped)
  Children.forEach(node, (section) => {
    if (!isValidElement(section)) return
    const sectionEl = section as ReactElement<{ children?: ReactNode }>
    const sectionType = String((sectionEl.type as { displayName?: string; name?: string } | string) || '')
      .toLowerCase()
    const isHead = sectionType.includes('thead') || sectionEl.type === 'thead'
    const isBody = sectionType.includes('tbody') || sectionEl.type === 'tbody'

    Children.forEach(sectionEl.props.children, (tr) => {
      if (!isValidElement(tr)) return
      const trEl = tr as ReactElement<{ children?: ReactNode }>
      const cells: string[] = []
      Children.forEach(trEl.props.children, (cell) => {
        if (!isValidElement(cell)) return
        cells.push(collectText((cell as ReactElement<{ children?: ReactNode }>).props.children).trim())
      })

      if (isHead) {
        cells.forEach((label, idx) => {
          columns.push({ key: String(idx), label })
        })
      } else if (isBody) {
        const row: TableRow = {}
        cells.forEach((text, idx) => {
          row[String(idx)] = text
        })
        rows.push(row)
      }
    })
  })

  return { columns, rows }
}

function FileLink({ href, children }: { href?: string; children?: ReactNode }) {
  const { t } = useTranslation()
  const isFileUrl = href?.startsWith('file:///')
  const isHttp = href?.startsWith('http://') || href?.startsWith('https://')

  if (isFileUrl) {
    const fileName = (() => {
      try {
        return decodeURIComponent(href!.slice(7)).split('/').pop() ?? ''
      } catch {
        return ''
      }
    })()
    return (
      <span
        role="link"
        tabIndex={0}
        title={t('common.openFile', 'Open file')}
        style={{
          cursor: 'pointer',
          textDecoration: 'underline',
          textDecorationStyle: 'dashed',
          textUnderlineOffset: 3,
          color: 'var(--color-primary)',
        }}
        onClick={() => {
          if (!fileName) return
          openFileByName(fileName).catch(() => {
            useNotificationStore.getState().push({
              level: 'error',
              title: t('chatArea.fileNotFound', 'File not found'),
              message: t('chatArea.cannotOpenFile', { fileName, defaultValue: `Cannot open ${fileName}` }),
              actions: [],
              dismissible: true,
              autoHide: 5,
              context: 'toast',
            })
          })
        }}
      >
        {children}
      </span>
    )
  }

  if (isHttp) {
    return (
      <a
        href={href}
        target="_blank"
        rel="noopener noreferrer"
        style={{ color: 'var(--color-primary)', textDecoration: 'underline' }}
      >
        {children}
      </a>
    )
  }

  return <>{children}</>
}

export const markdownComponents: Components = {
  table({ children }) {
    const { columns, rows } = extractTableFromGfm(children)
    if (columns.length === 0) {
      // Could not parse — let react-markdown fall back to a default <table>
      console.warn('[AssistantMarkdown] Could not extract table; falling back to native render.')
      return <table>{children}</table>
    }
    return (
      <div className="my-3">
        <TableView columns={columns} rows={rows} />
      </div>
    )
  },
  a({ href, children }) {
    return <FileLink href={href}>{children}</FileLink>
  },
  code: MarkdownCodeBlock,
}
