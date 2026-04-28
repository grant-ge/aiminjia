import type { ReactNode } from 'react'

interface ToolTraceIOProps {
  inputJson?: string
  output?: ReactNode
  isError?: boolean
}

export function ToolTraceIO({ inputJson, output, isError = false }: ToolTraceIOProps) {
  if (!inputJson && !output) return null

  return (
    <div
      className="flex flex-col gap-3 px-4 pb-4 pt-1"
      style={{ background: 'var(--color-bg-card)' }}
    >
      {inputJson ? (
        <div className="flex flex-col gap-1.5">
          <div className="text-xs font-semibold" style={{ color: 'var(--color-text-muted)' }}>
            输入
          </div>
          <pre
            className="whitespace-pre-wrap rounded-md p-3 font-mono text-xs leading-relaxed"
            style={{
              background: 'var(--color-bg-code)',
              color: 'var(--color-text-code)',
            }}
          >
            {inputJson}
          </pre>
        </div>
      ) : null}
      {output ? (
        <div className="flex flex-col gap-1.5">
          <div className="text-xs font-semibold" style={{ color: 'var(--color-text-muted)' }}>
            输出
          </div>
          <div
            className="rounded-md p-3 text-xs leading-relaxed"
            style={{
              background: isError
                ? 'var(--color-semantic-red-bg-light)'
                : 'var(--color-bg-neutral)',
              color: isError ? 'var(--color-semantic-red)' : 'var(--color-text-secondary)',
            }}
          >
            {output}
          </div>
        </div>
      ) : null}
    </div>
  )
}
