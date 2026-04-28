import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { SubAgentEnvelopeContent } from '@/types/message'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (
      key: string,
      fallbackOrOptions?: string | { defaultValue?: string; count?: number },
    ) => {
      if (typeof fallbackOrOptions === 'string') return fallbackOrOptions
      return fallbackOrOptions?.defaultValue ?? key
    },
  }),
}))

vi.mock('@/components/rich-content/SubAgentTranscriptViewer', () => ({
  SubAgentTranscriptViewer: ({ transcriptRef, variant }: { transcriptRef?: string; variant?: string }) =>
    transcriptRef ? <div data-testid="transcript-viewer">{transcriptRef}:{variant}</div> : null,
}))

import { SubAgentResultCard } from './SubAgentResultCard'

const baseEnvelope: SubAgentEnvelopeContent = {
  schemaVersion: 1,
  output: 'Completed the analysis task.',
  iterationsUsed: 3,
  generatedFiles: ['report.xlsx', 'chart.png'],
  transcriptRef: 'subagent://child-run-42',
}

describe('SubAgentResultCard', () => {
  it('renders the output text', () => {
    render(<SubAgentResultCard envelope={baseEnvelope} />)
    expect(screen.getByText('Completed the analysis task.')).toBeInTheDocument()
  })

  it('renders each generated file name', () => {
    render(<SubAgentResultCard envelope={baseEnvelope} />)
    expect(screen.getByText('report.xlsx')).toBeInTheDocument()
    expect(screen.getByText('chart.png')).toBeInTheDocument()
  })

  it('renders iteration count', () => {
    render(<SubAgentResultCard envelope={baseEnvelope} />)
    expect(screen.getByText(/3/)).toBeInTheDocument()
  })

  it('passes transcriptRef to SubAgentTranscriptViewer after expanding', () => {
    render(<SubAgentResultCard envelope={baseEnvelope} />)
    expect(screen.queryByTestId('transcript-viewer')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: /View execution trace/ }))

    expect(screen.getByTestId('transcript-viewer')).toHaveTextContent(
      'subagent://child-run-42:content',
    )
  })

  it('does not render transcript viewer when transcriptRef is absent', () => {
    render(
      <SubAgentResultCard
        envelope={{ ...baseEnvelope, transcriptRef: undefined }}
      />,
    )
    expect(screen.queryByTestId('transcript-viewer')).not.toBeInTheDocument()
  })

  it('does not render file list section when generatedFiles is empty', () => {
    render(
      <SubAgentResultCard
        envelope={{ ...baseEnvelope, generatedFiles: [] }}
      />,
    )
    expect(screen.queryByText('report.xlsx')).not.toBeInTheDocument()
  })
})
