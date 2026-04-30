import { useTranslation } from 'react-i18next'

import type { SubAgentEnvelopeContent } from '@/types/message'

import { ExecutionTraceCard } from './ExecutionTraceCard'
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
    <ExecutionTraceCard
      title={t('subagent.resultCard.header', 'Subagent Result')}
      badge={iterationsLabel}
      summary={<p>{output}</p>}
      sections={generatedFiles.length > 0 ? [
        {
          title: t('subagent.resultCard.filesGenerated', 'Files generated'),
          items: generatedFiles,
        },
      ] : undefined}
      expandLabel={transcriptRef ? t('subagent.transcript.expand', 'View execution trace') : undefined}
      collapseLabel={transcriptRef ? t('subagent.transcript.collapse', 'Hide execution trace') : undefined}
      expandedContent={transcriptRef ? <SubAgentTranscriptViewer transcriptRef={transcriptRef} variant="content" /> : undefined}
    />
  )
}
