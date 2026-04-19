import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { SubAgentTranscriptEntry } from '@/types/message'

const coreMock = vi.hoisted(() => ({
  invoke: vi.fn(),
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: coreMock.invoke,
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}))

import { getSubagentTranscript } from './tauri'

describe('getSubagentTranscript', () => {
  beforeEach(() => {
    coreMock.invoke.mockReset()
  })

  it('invokes get_subagent_transcript with the transcript ref', async () => {
    const entries: SubAgentTranscriptEntry[] = [
      { role: 'assistant', content: 'Analysis complete.' },
      { role: 'tool', content: 'Saved report.xlsx', toolName: 'execute_python' },
    ]
    coreMock.invoke.mockResolvedValue(entries)

    const result = await getSubagentTranscript('subagent://child-run-42')

    expect(coreMock.invoke).toHaveBeenCalledWith('get_subagent_transcript', {
      transcriptRef: 'subagent://child-run-42',
    })
    expect(result).toHaveLength(2)
    expect(result[1].toolName).toBe('execute_python')
  })

  it('returns empty array when backend returns []', async () => {
    coreMock.invoke.mockResolvedValue([])

    const result = await getSubagentTranscript('subagent://child-run-empty')

    expect(result).toEqual([])
  })
})
