/**
 * @designSource design.pen#8FxYa detail
 * @sizing bg muted padding [14,16,16,46] font mono
 */
import type { ReactNode } from 'react'

interface ToolGroupCodeBlockProps {
  inputJson?: string
  output?: ReactNode
}

export function ToolGroupCodeBlock({ inputJson, output }: ToolGroupCodeBlockProps) {
  return (
    <div className="flex flex-col gap-3 bg-muted px-14 py-4">
      {inputJson ? (
        <div className="flex flex-col gap-1.5">
          <div className="text-xs font-semibold text-muted-foreground">输入</div>
          {/* 代码块固定深色底 + 浅色字，跨 light/dark 主题保持终端样观感 */}
          <pre className="whitespace-pre-wrap rounded-md bg-[#0a0a0a] p-3 font-mono text-xs leading-relaxed text-primary-foreground">
            {inputJson}
          </pre>
        </div>
      ) : null}
      {output ? (
        <div className="flex flex-col gap-1.5">
          <div className="text-xs font-semibold text-muted-foreground">输出</div>
          <div>{output}</div>
        </div>
      ) : null}
    </div>
  )
}
