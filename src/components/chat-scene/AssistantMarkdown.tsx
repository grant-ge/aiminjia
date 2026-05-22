import ReactMarkdown from 'react-markdown'
import rehypeHighlight from 'rehype-highlight'
import remarkGfm from 'remark-gfm'
import { markdownComponents } from './markdown/markdownComponents'

interface AssistantMarkdownProps {
  text: string
  /**
   * Disable rehype-highlight (syntax highlighting).
   *
   * Why: streaming 时每帧把累积全段 markdown 重新 token 化是主要卡顿源。
   * 关闭后代码块仍以等宽灰色 <code> 渲染，done 后由 AiBubble 接管再开高亮。
   */
  disableCodeHighlight?: boolean
}

const REMARK_PLUGINS = [remarkGfm]
const REHYPE_PLUGINS_WITH_HIGHLIGHT: Parameters<typeof ReactMarkdown>[0]['rehypePlugins'] = [
  [rehypeHighlight, { detect: true }],
]
const REHYPE_PLUGINS_NO_HIGHLIGHT: Parameters<typeof ReactMarkdown>[0]['rehypePlugins'] = []

export function AssistantMarkdown({ text, disableCodeHighlight = false }: AssistantMarkdownProps) {
  if (!text.trim()) return null

  return (
    <div className="assistant-markdown text-[15px] leading-7 tracking-[-0.003em]">
      <ReactMarkdown
        remarkPlugins={REMARK_PLUGINS}
        rehypePlugins={
          disableCodeHighlight ? REHYPE_PLUGINS_NO_HIGHLIGHT : REHYPE_PLUGINS_WITH_HIGHLIGHT
        }
        skipHtml
        components={markdownComponents}
      >
        {text}
      </ReactMarkdown>
    </div>
  )
}
