import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { SubAgentTranscriptEntry } from '@/types/message'

const mockGetSubagentTranscript = vi.hoisted(() =>
  vi.fn<(ref: string) => Promise<SubAgentTranscriptEntry[]>>(),
)

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

vi.mock('@/lib/tauri', () => ({
  getSubagentTranscript: mockGetSubagentTranscript,
}))

import { SubAgentTranscriptViewer } from './SubAgentTranscriptViewer'

const ENTRIES: SubAgentTranscriptEntry[] = [
  { role: 'assistant', content: 'Running analysis...' },
  {
    role: 'tool',
    content: 'Wrote 3 rows to report.xlsx',
    toolName: 'execute_python',
  },
  { role: 'assistant', content: 'Done.' },
]

describe('SubAgentTranscriptViewer', () => {
  beforeEach(() => {
    mockGetSubagentTranscript.mockReset()
  })

  it('renders a toggle button initially (collapsed)', () => {
    render(<SubAgentTranscriptViewer transcriptRef="subagent://run-1" />)
    expect(screen.getByRole('button')).toBeInTheDocument()
    expect(screen.queryByText('Running analysis...')).not.toBeInTheDocument()
  })

  it('loads transcript and renders entries on first expand', async () => {
    mockGetSubagentTranscript.mockResolvedValue(ENTRIES)

    render(<SubAgentTranscriptViewer transcriptRef="subagent://run-2" />)
    fireEvent.click(screen.getByRole('button'))

    await waitFor(() => {
      expect(screen.getByText('Running analysis...')).toBeInTheDocument()
    })

    expect(screen.getByText('Wrote 3 rows to report.xlsx')).toBeInTheDocument()
    expect(screen.getByText('execute_python')).toBeInTheDocument()
    expect(mockGetSubagentTranscript).toHaveBeenCalledTimes(1)
    expect(mockGetSubagentTranscript).toHaveBeenCalledWith('subagent://run-2')
  })

  it('does not re-fetch on second expand after collapsing', async () => {
    mockGetSubagentTranscript.mockResolvedValue(ENTRIES)

    render(<SubAgentTranscriptViewer transcriptRef="subagent://run-3" />)

    fireEvent.click(screen.getByRole('button'))
    await waitFor(() => {
      expect(screen.getByText('Running analysis...')).toBeInTheDocument()
    })

    fireEvent.click(screen.getByRole('button'))
    expect(screen.queryByText('Running analysis...')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button'))
    expect(screen.getByText('Running analysis...')).toBeInTheDocument()
    expect(mockGetSubagentTranscript).toHaveBeenCalledTimes(1)
  })

  it('shows an error message when the fetch fails', async () => {
    mockGetSubagentTranscript.mockRejectedValue(new Error('transcript not found'))

    render(<SubAgentTranscriptViewer transcriptRef="subagent://run-bad" />)
    fireEvent.click(screen.getByRole('button'))

    await waitFor(() => {
      expect(screen.getByRole('alert')).toBeInTheDocument()
    })
  })

  it('retries loading after an initial failure when expanded again', async () => {
    mockGetSubagentTranscript
      .mockRejectedValueOnce(new Error('transient error'))
      .mockResolvedValueOnce(ENTRIES)

    render(<SubAgentTranscriptViewer transcriptRef="subagent://run-retry" />)

    fireEvent.click(screen.getByRole('button'))
    await waitFor(() => {
      expect(screen.getByRole('alert')).toBeInTheDocument()
    })

    fireEvent.click(screen.getByRole('button'))
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button'))
    await waitFor(() => {
      expect(screen.getByText('Running analysis...')).toBeInTheDocument()
    })

    expect(mockGetSubagentTranscript).toHaveBeenCalledTimes(2)
  })

  it('displays role badges for each entry', async () => {
    mockGetSubagentTranscript.mockResolvedValue(ENTRIES)

    render(<SubAgentTranscriptViewer transcriptRef="subagent://run-4" />)
    fireEvent.click(screen.getByRole('button'))

    await waitFor(() => {
      expect(screen.getByText('Running analysis...')).toBeInTheDocument()
    })

    expect(screen.getAllByText('assistant')).toHaveLength(2)
    expect(screen.getByText('tool')).toBeInTheDocument()
  })

  it('renders transcript content directly in content variant', async () => {
    mockGetSubagentTranscript.mockResolvedValue(ENTRIES)

    render(<SubAgentTranscriptViewer transcriptRef="subagent://run-inline" variant="content" />)

    expect(screen.queryByRole('button')).not.toBeInTheDocument()
    await waitFor(() => {
      expect(screen.getByText('Running analysis...')).toBeInTheDocument()
    })
    expect(mockGetSubagentTranscript).toHaveBeenCalledWith('subagent://run-inline')
  })

  it('does not auto-retry content variant after a load failure', async () => {
    mockGetSubagentTranscript.mockRejectedValue(new Error('transcript not found'))

    render(<SubAgentTranscriptViewer transcriptRef="subagent://run-inline-bad" variant="content" />)

    await waitFor(() => {
      expect(screen.getByRole('alert')).toBeInTheDocument()
    })
    await new Promise((resolve) => window.setTimeout(resolve, 10))
    expect(mockGetSubagentTranscript).toHaveBeenCalledTimes(1)
  })

})
