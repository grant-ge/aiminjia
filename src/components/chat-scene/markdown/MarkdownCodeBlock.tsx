import { useState, useCallback } from 'react'
import { useTranslation } from 'react-i18next'

interface CodeProps {
  inline?: boolean
  className?: string
  children?: React.ReactNode
}

function textFromNode(node: React.ReactNode): string {
  if (node == null || typeof node === 'boolean') return ''
  if (typeof node === 'string' || typeof node === 'number') return String(node)
  if (Array.isArray(node)) return node.map(textFromNode).join('')
  if (typeof node === 'object' && 'props' in node) {
    return textFromNode((node as React.ReactElement<{ children?: React.ReactNode }>).props.children)
  }
  return ''
}

function InlineCode({ children }: { children?: React.ReactNode }) {
  return (
    <code
      style={{
        background: 'var(--color-bg-base)',
        padding: '1px 5px',
        borderRadius: 'var(--radius-md)',
        fontFamily: 'var(--font-mono)',
        fontSize: '0.928571em',
        color: 'var(--color-text-primary)',
      }}
    >
      {children}
    </code>
  )
}

function FencedCodeBlock({ className, children }: { className?: string; children?: React.ReactNode }) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState<'idle' | 'ok' | 'fail'>('idle')

  const match = /language-(\w+)/.exec(className ?? '')
  const lang = match?.[1] ?? 'code'
  const codeText = textFromNode(children).replace(/\n$/, '')

  const handleCopy = useCallback(() => {
    navigator.clipboard
      .writeText(codeText)
      .then(() => {
        setCopied('ok')
        setTimeout(() => setCopied('idle'), 2000)
      })
      .catch(() => {
        setCopied('fail')
        setTimeout(() => setCopied('idle'), 2000)
      })
  }, [codeText])

  return (
    <div
      style={{
        margin: '12px 0',
        borderRadius: 'var(--radius-md)',
        overflow: 'hidden',
        border: '1px solid var(--color-border-subtle)',
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          height: 32,
          boxSizing: 'border-box',
          padding: '0 12px',
          background: 'var(--color-bg-base)',
          fontSize: '0.75rem',
          color: 'var(--color-text-muted)',
          fontFamily: 'var(--font-mono)',
        }}
      >
        <span>{lang}</span>
        <button
          type="button"
          onClick={handleCopy}
          style={{
            cursor: 'pointer',
            border: 'none',
            background: 'none',
            fontSize: '0.7rem',
            color:
              copied === 'ok'
                ? 'var(--color-semantic-green)'
                : copied === 'fail'
                  ? 'var(--color-semantic-red)'
                  : 'var(--color-text-muted)',
            fontFamily: 'var(--font-mono)',
            padding: '2px 6px',
            borderRadius: 'var(--radius-md)',
          }}
        >
          {copied === 'ok'
            ? t('common.copied', 'Copied')
            : copied === 'fail'
              ? t('common.copyFailed', 'Copy failed')
              : t('common.copy', 'Copy')}
        </button>
      </div>
      <pre
        style={{
          margin: 0,
          padding: '12px 14px',
          overflowX: 'auto',
          background: 'var(--color-bg-elevated, var(--color-bg-card))',
          fontSize: '0.928571em',
          lineHeight: 1.55,
          fontFamily: 'var(--font-mono)',
          color: 'var(--color-text-primary)',
        }}
      >
        <code className={className}>{children}</code>
      </pre>
    </div>
  )
}

/**
 * react-markdown `code` override.
 * Renders inline code as <code>; fenced code blocks as a card with a copy button.
 */
export function MarkdownCodeBlock({ inline, className, children }: CodeProps) {
  const rawCodeText = textFromNode(children)
  const isFenced = inline === false || Boolean(className) || rawCodeText.includes('\n')
  if (!isFenced) return <InlineCode>{children}</InlineCode>
  return <FencedCodeBlock className={className}>{children}</FencedCodeBlock>
}
