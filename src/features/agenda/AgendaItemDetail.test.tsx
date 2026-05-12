import '@testing-library/jest-dom'
import { render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { AgendaItem } from '@/lib/tauri'

const invokeMock = vi.hoisted(() => vi.fn())
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

import { AgendaItemDetail } from './AgendaItemDetail'

const sampleItem: AgendaItem = {
  id: 'agenda-1',
  title: 'T',
  prompt: 'P',
  organizerEmployeeId: 'emp-1',
  participants: [],
  startAt: '2026-05-07T01:00:00Z',
  timezone: 'Asia/Shanghai',
  rule: null,
  skipDates: [],
  nextFireAt: '2026-05-07T01:00:00Z',
  occurrenceCount: 0,
  status: 'active',
  overrideOf: null,
  workspacePath: null,
  createdAt: '',
  updatedAt: '',
}

describe('AgendaItemDetail', () => {
  beforeEach(() => {
    invokeMock.mockReset()
  })

  it('loads occurrences when opened', async () => {
    invokeMock.mockResolvedValueOnce([
      {
        id: 'occ-1',
        agendaItemId: 'agenda-1',
        firedAt: '2026-05-06T01:00:00Z',
        plannedFireAt: '2026-05-06T01:00:00Z',
        startedAt: '2026-05-06T01:00:00Z',
        finishedAt: '2026-05-06T01:01:00Z',
        primaryEmployeeId: 'emp-1',
        conversationId: 'conv-1',
        sessionId: 'conv-1',
        runId: 'run-1',
        status: 'succeeded',
        errorSummary: null,
        triggerSource: 'scheduled',
      },
    ])

    render(
      <AgendaItemDetail
        open
        item={sampleItem}
        onClose={() => {}}
        onChanged={() => {}}
      />,
    )

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('list_agenda_occurrences', {
        itemId: 'agenda-1',
        limit: 50,
      })
    })
  })

  it('renders overview tab fields', () => {
    invokeMock.mockResolvedValueOnce([])
    render(
      <AgendaItemDetail
        open
        item={sampleItem}
        onClose={() => {}}
        onChanged={() => {}}
      />,
    )
    expect(screen.getByText(/组织者/)).toBeInTheDocument()
    expect(screen.getByText(/频率/)).toBeInTheDocument()
  })
})
