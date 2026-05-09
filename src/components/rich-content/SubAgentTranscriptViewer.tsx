import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { getSubagentTranscript } from '@/lib/tauri'
import type { SubAgentTranscriptEntry } from '@/types/message'

interface SubAgentTranscriptViewerProps {
  transcriptRef: string
  variant?: 'toggle' | 'content'
}

type LoadState = 'idle' | 'loading' | 'loaded' | 'error'

const ROLE_BADGE_STYLE: Record<string, { bg: string; color: string }> = {
  assistant: {
    bg: 'var(--color-filetype-blue-bg)',
    color: 'var(--color-semantic-blue)',
  },
  tool: {
    bg: 'var(--color-filetype-green-bg)',
    color: 'var(--color-semantic-green)',
  },
  user: {
    bg: 'var(--color-bg-neutral)',
    color: 'var(--color-text-muted)',
  },
}

function roleBadgeStyle(role: string) {
  return ROLE_BADGE_STYLE[role] ?? ROLE_BADGE_STYLE.user
}

function TranscriptBody({
  loadState,
  entries,
  errorMsg,
}: {
  loadState: LoadState
  entries: SubAgentTranscriptEntry[]
  errorMsg: string
}) {
  const { t } = useTranslation()

  return (
    <div className="px-4 py-3">
      {loadState === 'loading' && (
        <div className="py-2 text-xs" style={{ color: 'var(--color-text-muted)' }}>
          {t('subagent.transcript.loading', 'Loading execution trace...')}
        </div>
      )}

      {loadState === 'error' && (
        <div
          role="alert"
          className="rounded-md px-3 py-2 text-xs"
          style={{
            background: 'var(--color-semantic-red-bg-light)',
            color: 'var(--color-semantic-red)',
          }}
        >
          {t('subagent.transcript.error', 'Failed to load execution trace')}: {errorMsg}
        </div>
      )}

      {loadState === 'loaded' && (
        <div className="flex flex-col gap-2">
          {entries.map((entry, index) => {
            const badgeStyle = roleBadgeStyle(entry.role)
            return (
              <div key={`${entry.role}-${index}`} className="flex gap-2.5">
                <span
                  className="mt-0.5 shrink-0 rounded px-1.5 py-0.5 text-xs font-semibold"
                  style={{
                    background: badgeStyle.bg,
                    color: badgeStyle.color,
                    alignSelf: 'flex-start',
                  }}
                >
                  {entry.role}
                </span>

                <div className="min-w-0 flex-1">
                  {entry.toolName && (
                    <div
                      className="mb-0.5 font-mono text-xs"
                      style={{ color: 'var(--color-text-muted)' }}
                    >
                      {entry.toolName}
                    </div>
                  )}
                  <p
                    className="whitespace-pre-wrap break-words text-xs leading-relaxed"
                    style={{ color: 'var(--color-text-secondary)' }}
                  >
                    {entry.content}
                  </p>
                </div>
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}

export function SubAgentTranscriptViewer({
  transcriptRef,
  variant = 'toggle',
}: SubAgentTranscriptViewerProps) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(false)
  const [loadState, setLoadState] = useState<LoadState>('idle')
  const [entries, setEntries] = useState<SubAgentTranscriptEntry[]>([])
  const [errorMsg, setErrorMsg] = useState('')

  const loadTranscript = useCallback(async () => {
    setLoadState('loading')
    setErrorMsg('')
    try {
      const loaded = await getSubagentTranscript(transcriptRef)
      setEntries(loaded)
      setLoadState('loaded')
    } catch (error) {
      setErrorMsg(error instanceof Error ? error.message : String(error))
      setLoadState('error')
    }
  }, [transcriptRef])

  useEffect(() => {
    if (variant === 'content' && loadState === 'idle') {
      const timeoutId = window.setTimeout(() => {
        void loadTranscript()
      }, 0)
      return () => window.clearTimeout(timeoutId)
    }
  }, [loadState, loadTranscript, variant])

  const handleToggle = useCallback(async () => {
    const shouldLoad = !expanded && (loadState === 'idle' || loadState === 'error')
    if (shouldLoad) {
      setExpanded(true)
      await loadTranscript()
      return
    }

    setExpanded((prev) => !prev)
  }, [expanded, loadState, loadTranscript])

  if (variant === 'content') {
    return <TranscriptBody loadState={loadState} entries={entries} errorMsg={errorMsg} />
  }

  return (
    <div>
      <button
        onClick={handleToggle}
        className="flex w-full items-center gap-2 px-4 py-2.5 text-left text-xs transition-colors hover:bg-[var(--color-bg-hover)]"
        style={{ color: 'var(--color-text-muted)' }}
      >
        <svg
          width="12"
          height="12"
          viewBox="0 0 12 12"
          fill="none"
          style={{
            transition: 'transform 0.15s ease',
            transform: expanded ? 'rotate(90deg)' : 'rotate(0deg)',
            flexShrink: 0,
          }}
        >
          <path
            d="M4 2l4 4-4 4"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>

        <span className="font-medium">
          {expanded
            ? t('subagent.transcript.collapse', 'Hide execution trace')
            : t('subagent.transcript.expand', 'View execution trace')}
        </span>

        {loadState === 'loaded' && (
          <span
            className="rounded-full px-1.5 py-0.5 font-medium"
            style={{
              background: 'var(--color-bg-neutral)',
              color: 'var(--color-text-muted)',
            }}
          >
            {entries.length}
          </span>
        )}
      </button>

      {expanded && (
        <div
          className="border-t border-border"
          style={{ borderColor: 'var(--color-border)' }}
        >
          <TranscriptBody loadState={loadState} entries={entries} errorMsg={errorMsg} />
        </div>
      )}
    </div>
  )
}
