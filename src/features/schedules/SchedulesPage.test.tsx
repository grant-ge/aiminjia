import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { AgendaItem } from '@/lib/tauri'

import { SchedulesPage } from './SchedulesPage'

const invokeMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}))

const sampleItem = (over: Partial<AgendaItem> = {}): AgendaItem => ({
  id: 'agenda-1',
  title: '测试日程',
  prompt: 'P',
  organizerPersonaId: 'p1',
  participants: [],
  startAt: '2026-05-07T01:00:00Z',
  timezone: 'Asia/Shanghai',
  rule: null,
  skipDates: [],
  nextFireAt: '2026-05-07T01:00:00Z',
  occurrenceCount: 0,
  status: 'active',
  overrideOf: null,
  createdAt: '',
  updatedAt: '',
  ...over,
})

interface InvokeQueueHandlers {
  agendaListSequence: AgendaItem[][]
  onUpdate?: (req: { id: string; request: unknown }) => Promise<unknown>
  onRunNow?: (req: { id: string }) => Promise<unknown>
  onDelete?: (req: { id: string }) => Promise<unknown>
}

function setupInvokeQueue(handlers: InvokeQueueHandlers) {
  let listIdx = 0
  invokeMock.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === 'get_active_persona') {
      return { id: 'p1', name: 'P1' }
    }
    if (cmd === 'get_persona') {
      return { id: 'p1', name: 'P1' }
    }
    if (cmd === 'list_agenda_items') {
      const next = handlers.agendaListSequence[Math.min(listIdx, handlers.agendaListSequence.length - 1)]
      listIdx += 1
      return next
    }
    if (cmd === 'update_agenda_item') {
      return handlers.onUpdate
        ? await handlers.onUpdate(args as { id: string; request: unknown })
        : null
    }
    if (cmd === 'run_agenda_item_now') {
      return handlers.onRunNow ? await handlers.onRunNow(args as { id: string }) : 'occ-x'
    }
    if (cmd === 'delete_agenda_item') {
      return handlers.onDelete ? await handlers.onDelete(args as { id: string }) : true
    }
    return null
  })
}

describe('SchedulesPage', () => {
  beforeEach(() => {
    invokeMock.mockReset()
  })

  it('loads agenda items and renders count', async () => {
    setupInvokeQueue({
      agendaListSequence: [[sampleItem({ title: '日报汇总' })]],
    })

    render(<SchedulesPage />)

    expect(await screen.findByText('日报汇总')).toBeTruthy()
    expect(screen.getByText('共 1 条')).toBeTruthy()
    expect(invokeMock).toHaveBeenCalledWith('list_agenda_items', { filter: undefined })
  })

  it('asks for confirmation before deleting an agenda item', async () => {
    const deleteCalls: Array<{ id: string }> = []
    setupInvokeQueue({
      agendaListSequence: [[sampleItem({ title: '日报汇总' })], []],
      onDelete: async (req) => {
        deleteCalls.push(req)
        return true
      },
    })

    render(<SchedulesPage />)
    await screen.findByText('日报汇总')

    fireEvent.click(screen.getByRole('button', { name: '删除 日报汇总' }))

    expect(screen.getByText('删除此定时任务？')).toBeInTheDocument()
    expect(deleteCalls).toHaveLength(0)

    fireEvent.click(screen.getByRole('button', { name: '确认删除' }))

    await waitFor(() => {
      expect(deleteCalls).toEqual([{ id: 'agenda-1' }])
    })
  })

  it('toggles status via row pause button', async () => {
    const updateCalls: Array<{ id: string; request: unknown }> = []
    setupInvokeQueue({
      agendaListSequence: [
        [sampleItem({ status: 'active' })],
        [sampleItem({ status: 'paused' })],
      ],
      onUpdate: async (req) => {
        updateCalls.push(req)
        return sampleItem({ status: 'paused' })
      },
    })

    render(<SchedulesPage />)
    await screen.findByText('测试日程')

    fireEvent.click(screen.getByRole('button', { name: /^暂停 测试日程$/ }))

    await waitFor(() => {
      expect(updateCalls).toEqual([
        { id: 'agenda-1', request: { status: 'paused' } },
      ])
    })
  })

  it('run-now button triggers run_agenda_item_now', async () => {
    const runCalls: Array<{ id: string }> = []
    setupInvokeQueue({
      agendaListSequence: [[sampleItem({})], [sampleItem({})]],
      onRunNow: async (req) => {
        runCalls.push(req)
        return 'occ-x'
      },
    })

    render(<SchedulesPage />)
    await screen.findByText('测试日程')

    fireEvent.click(screen.getByRole('button', { name: /^立即运行 测试日程$/ }))

    await waitFor(() => {
      expect(runCalls).toEqual([{ id: 'agenda-1' }])
    })
  })
})
