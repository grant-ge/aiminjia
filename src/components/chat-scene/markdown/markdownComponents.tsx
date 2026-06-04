import type { Components } from 'react-markdown'
import { MarkdownCodeBlock } from './MarkdownCodeBlock'
import { MarkdownTable } from './MarkdownTable'
import { FileLink, FileImage } from './FileLink'

interface MarkdownComponentOptions {
  conversationId?: string
  workspaceRoot?: string
}

export function createMarkdownComponents({
  conversationId,
  workspaceRoot,
}: MarkdownComponentOptions = {}): Components {
  return {
    pre({ children }) {
      return <>{children}</>
    },
    table({ children }) {
      return <MarkdownTable>{children}</MarkdownTable>
    },
    a({ href, children }) {
      return (
        <FileLink href={href} conversationId={conversationId} workspaceRoot={workspaceRoot}>
          {children}
        </FileLink>
      )
    },
    img({ src, alt }) {
      return (
        <FileImage
          src={typeof src === 'string' ? src : undefined}
          alt={typeof alt === 'string' ? alt : ''}
          conversationId={conversationId}
          workspaceRoot={workspaceRoot}
        />
      )
    },
    code: MarkdownCodeBlock,
  }
}

export const markdownComponents: Components = createMarkdownComponents()
