import { markdownToHtml } from '@/lib/markdown'

interface AssistantMarkdownProps {
  text: string
}

export function AssistantMarkdown({ text }: AssistantMarkdownProps) {
  if (!text.trim()) return null

  return (
    <div
      className="assistant-markdown text-[15px] leading-7"
      dangerouslySetInnerHTML={{ __html: markdownToHtml(text) }}
    />
  )
}
