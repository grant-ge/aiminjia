import type { Components } from 'react-markdown'
import { MarkdownCodeBlock } from './MarkdownCodeBlock'
import { MarkdownTable } from './MarkdownTable'
import { FileLink } from './FileLink'

export const markdownComponents: Components = {
  pre({ children }) {
    return <>{children}</>
  },
  table({ children }) {
    return <MarkdownTable>{children}</MarkdownTable>
  },
  a({ href, children }) {
    return <FileLink href={href}>{children}</FileLink>
  },
  code: MarkdownCodeBlock,
}
