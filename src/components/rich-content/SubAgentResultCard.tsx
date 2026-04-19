import { useTranslation } from 'react-i18next'

import type { SubAgentEnvelopeContent } from '@/types/message'

import { SubAgentTranscriptViewer } from './SubAgentTranscriptViewer'

interface SubAgentResultCardProps {
  envelope: SubAgentEnvelopeContent
}

export function SubAgentResultCard({ envelope }: SubAgentResultCardProps) {
  const { t } = useTranslation()
  const { output, generatedFiles, iterationsUsed, transcriptRef } = envelope
  const iterationsLabel = t('subagent.resultCard.iterations', {
    count: iterationsUsed,
    defaultValue: `${iterationsUsed} iterations`,
  })

  return (
    <div
      className="my-2 rounded-lg border"
      style={{
        background: 'var(--color-bg-card)',
        borderColor: 'var(--color-border)',
      }}
    >
      <div
        className="flex items-center justify-between rounded-t-lg border-b px-4 py-2.5"
        style={{
          background: 'var(--color-bg-elevated)',
          borderColor: 'var(--color-border)',
        }}
      >
        <span
          className="text-xs font-semibold uppercase tracking-wide"
          style={{ color: 'var(--color-text-muted)' }}
        >
          {t('subagent.resultCard.header', 'Subagent Result')}
        </span>
        <span
          className="rounded-full px-2 py-0.5 text-xs font-medium"
          style={{
            background: 'var(--color-bg-neutral)',
            color: 'var(--color-text-muted)',
          }}
        >
          {iterationsLabel}
        </span>
      </div>

      <div className="px-4 py-3">
        <p
          className="text-sm leading-relaxed"
          style={{ color: 'var(--color-text-primary)' }}
        >
          {output}
        </p>
      </div>

      {generatedFiles.length > 0 && (
        <div
          className="border-t px-4 py-2.5"
          style={{ borderColor: 'var(--color-border)' }}
        >
          <div
            className="mb-1.5 text-xs font-semibold uppercase tracking-wide"
            style={{ color: 'var(--color-text-muted)' }}
          >
            {t('subagent.resultCard.filesGenerated', 'Files generated')}
          </div>
          <div className="flex flex-wrap gap-1.5">
            {generatedFiles.map((name) => (
              <span
                key={name}
                className="rounded-md px-2 py-0.5 text-xs font-medium"
                style={{
                  background: 'var(--color-bg-neutral)',
                  color: 'var(--color-text-secondary)',
                  fontFamily: 'monospace',
                }}
              >
                {name}
              </span>
            ))}
          </div>
        </div>
      )}

      {transcriptRef && (
        <div className="border-t" style={{ borderColor: 'var(--color-border)' }}>
          <SubAgentTranscriptViewer transcriptRef={transcriptRef} />
        </div>
      )}
    </div>
  )
}
