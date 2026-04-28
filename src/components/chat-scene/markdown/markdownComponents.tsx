import type { Components } from 'react-markdown'
import { TableView } from '@/components/data-table'
import { MarkdownCodeBlock } from './MarkdownCodeBlock'
import { extractTableFromGfm } from './extractTableFromGfm'
import { FileLink } from './FileLink'

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
