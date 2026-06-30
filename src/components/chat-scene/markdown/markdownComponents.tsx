import type { Components } from 'react-markdown'
import { Children, isValidElement } from 'react'
import { AijiaCardCodeBlock } from '@/components/chat-scene/result-cards/AijiaCardCodeBlock'
import { MarkdownTable } from './MarkdownTable'
import { FileLink, FileImage } from './FileLink'
import type { GeneratedFile } from '@/types/message'
import { ARTIFACT_ALT } from './artifactMarkdown'

interface MarkdownComponentOptions {
  conversationId?: string
  workspaceRoot?: string
  generatedFiles?: GeneratedFile[]
}

export function createMarkdownComponents({
  conversationId,
  workspaceRoot,
  generatedFiles,
}: MarkdownComponentOptions = {}): Components {
  return {
    p({ children }) {
      const hasArtifactImage = Children.toArray(children).some(
        (child) => isValidElement(child) && child.props.alt === ARTIFACT_ALT,
      )
      return hasArtifactImage ? <div>{children}</div> : <p>{children}</p>
    },
    pre({ children }) {
      return <>{children}</>
    },
    table({ children }) {
      return <MarkdownTable>{children}</MarkdownTable>
    },
    a({ href, children }) {
      return (
        <FileLink
          href={href}
          conversationId={conversationId}
          workspaceRoot={workspaceRoot}
          generatedFiles={generatedFiles}
        >
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
          generatedFiles={generatedFiles}
        />
      )
    },
    code: AijiaCardCodeBlock,
  }
}

export const markdownComponents: Components = createMarkdownComponents()
