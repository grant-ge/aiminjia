import ReactMarkdown from 'react-markdown'
import rehypeHighlight from 'rehype-highlight'
import remarkGfm from 'remark-gfm'
import { markdownComponents } from './markdown/markdownComponents'

interface AssistantMarkdownProps {
  text: string
}

export function AssistantMarkdown({ text }: AssistantMarkdownProps) {
  if (!text.trim()) return null

  return (
    <div className="assistant-markdown text-sm leading-7">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[[rehypeHighlight, { detect: true }]]}
        skipHtml
        components={markdownComponents}
      >
        {text}
      </ReactMarkdown>
    </div>
  )
}
