/**
 * RichCodeBlock — syntax-highlighted code block with header.
 * Based on visual-prototype-zh.html .code-block styles.
 */
import { useState, useCallback } from 'react'
import type { CodeBlock, CodeResult } from '@/types/message'

interface RichCodeBlockProps {
  block: CodeBlock
  result?: CodeResult
}

const STATUS_INDICATOR: Record<CodeBlock['status'], { label: string; color: string }> = {
  pending: { label: 'Pending', color: 'var(--color-text-muted)' },
  running: { label: 'Running...', color: 'var(--color-semantic-blue)' },
  success: { label: 'Done', color: 'var(--color-semantic-green)' },
  error: { label: 'Error', color: 'var(--color-semantic-red)' },
}

export function RichCodeBlock({ block, result }: RichCodeBlockProps) {
  const status = STATUS_INDICATOR[block.status]
  const [copied, setCopied] = useState(false)

  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(block.code).then(() => {
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    })
  }, [block.code])

  return (
    <div
      className="my-3 overflow-hidden rounded-lg border"
      style={{
        background: 'var(--color-bg-code)',
        borderColor: 'var(--color-border)',
      }}
    >
      {/* Header */}
      <div
        className="flex items-center justify-between border-b px-3.5 py-2"
        style={{
          background: 'var(--color-bg-code-header)',
          borderColor: 'var(--color-border)',
        }}
      >
        <span
          className="flex items-center gap-1.5 text-xs font-semibold"
          style={{ color: 'var(--color-text-muted)' }}
        >
          {block.language}
          {block.purpose && <span className="font-normal"> — {block.purpose}</span>}
        </span>
        <div className="flex items-center gap-2">
          <button
            onClick={handleCopy}
            className="flex items-center gap-1 text-xs transition-colors"
            style={{ color: copied ? 'var(--color-semantic-green)' : 'var(--color-text-muted)' }}
          >
            {copied ? '已复制' : '复制'}
          </button>
          <span className="text-xs font-medium" style={{ color: status.color }}>
            {status.label}
          </span>
        </div>
      </div>

      {/* Code body */}
      <pre
        className="overflow-x-auto whitespace-pre px-3.5 py-3 font-mono text-sm leading-relaxed"
        style={{ color: 'var(--color-text-code)' }}
      >
        {block.code}
      </pre>

      {/* Result output */}
      {result && (
        <div
          className="border-t px-3.5 py-2.5 font-mono text-sm leading-[1.6]"
          style={{
            borderColor: 'var(--color-border)',
            background: result.isError ? 'var(--color-semantic-red-bg-light)' : 'var(--color-semantic-green-bg-light)',
            color: result.isError ? 'var(--color-semantic-red)' : 'var(--color-text-secondary)',
          }}
        >
          <pre className="whitespace-pre-wrap">{result.output}</pre>
        </div>
      )}
    </div>
  )
}
