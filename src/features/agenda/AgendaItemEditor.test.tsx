import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const invokeMock = vi.hoisted(() => vi.fn())
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

import { AgendaItemEditor } from './AgendaItemEditor'

describe('AgendaItemEditor', () => {
  beforeEach(() => {
    invokeMock.mockReset()
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_default_folder') {
        return { id: 'default', rootPath: '/tmp/default', displayName: 'default' }
      }
      if (cmd === 'list_personas') {
        return [
          {
            id: 'p1',
            name: '小一',
            nameEn: 'p1',
            icon: '',
            description: '',
            descriptionEn: '',
            builtin: true,
          },
          {
            id: 'p2',
            name: '小二',
            nameEn: 'p2',
            icon: '',
            description: '',
            descriptionEn: '',
            builtin: true,
          },
        ]
      }
      return null
    })
  })

  it('saves new one-shot item', async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_default_folder') {
        return { id: 'default', rootPath: '/tmp/default', displayName: 'default' }
      }
      if (cmd === 'list_personas') {
        return [
          {
            id: 'p1',
            name: '小一',
            nameEn: 'p1',
            icon: '',
            description: '',
            descriptionEn: '',
            builtin: true,
          },
        ]
      }
      if (cmd === 'create_agenda_item') {
        return { id: 'agenda-x' }
      }
      return null
    })
    const onSaved = vi.fn()

    render(
      <AgendaItemEditor
        open
        organizerPersonaId="p1"
        onClose={() => {}}
        onSaved={onSaved}
      />,
    )

    fireEvent.change(screen.getByPlaceholderText('标题'), {
      target: { value: 'T' },
    })
    fireEvent.change(screen.getByPlaceholderText('到点要做什么？'), {
      target: { value: 'P' },
    })

    const dt = screen.getByLabelText(/开始时间/) as HTMLInputElement
    fireEvent.change(dt, { target: { value: '2026-05-07T09:00' } })

    fireEvent.click(screen.getByRole('button', { name: '保存' }))

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        'create_agenda_item',
        expect.objectContaining({
          request: expect.objectContaining({
            title: 'T',
            prompt: 'P',
            organizerPersonaId: 'p1',
          }),
        }),
      )
      expect(onSaved).toHaveBeenCalled()
    })
  })

  it('renders frequency-conditional fields when freq != one_shot', async () => {
    render(
      <AgendaItemEditor
        open
        organizerPersonaId="p1"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    )
    expect(screen.queryByLabelText('结束条件')).toBeNull()
    fireEvent.change(screen.getByLabelText('频率'), {
      target: { value: 'daily' },
    })
    await waitFor(() =>
      expect(screen.getByLabelText('结束条件')).toBeInTheDocument(),
    )
  })

  it('defaults organizer to the active persona when creating', async () => {
    render(
      <AgendaItemEditor
        open
        organizerPersonaId="p2"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    )

    const select = (await screen.findByLabelText('执行员工')) as HTMLSelectElement
    expect(select.value).toBe('p2')
    expect(select.disabled).toBe(false)
  })

  it('passes the chosen persona id when creating', async () => {
    const onSaved = vi.fn()

    render(
      <AgendaItemEditor
        open
        organizerPersonaId="p1"
        onClose={() => {}}
        onSaved={onSaved}
      />,
    )

    const select = (await screen.findByLabelText('执行员工')) as HTMLSelectElement
    await waitFor(() => expect(select.querySelectorAll('option').length).toBeGreaterThan(1))
    fireEvent.change(select, { target: { value: 'p2' } })

    fireEvent.change(screen.getByPlaceholderText('标题'), { target: { value: 'T' } })
    fireEvent.change(screen.getByPlaceholderText('到点要做什么？'), {
      target: { value: 'P' },
    })
    fireEvent.change(screen.getByLabelText(/开始时间/), {
      target: { value: '2026-05-07T09:00' },
    })

    fireEvent.click(screen.getByRole('button', { name: '保存' }))

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        'create_agenda_item',
        expect.objectContaining({
          request: expect.objectContaining({ organizerPersonaId: 'p2' }),
        }),
      )
      expect(onSaved).toHaveBeenCalled()
    })
  })

  it('locks organizer when editing an existing item', async () => {
    const item = {
      id: 'a1',
      title: 'X',
      prompt: 'Y',
      startAt: '2026-05-07T01:00:00.000Z',
      timezone: 'Asia/Shanghai',
      organizerPersonaId: 'p2',
      rule: null,
      workspacePath: null,
    } as unknown as Parameters<typeof AgendaItemEditor>[0]['initial']

    render(
      <AgendaItemEditor
        open
        initial={item}
        organizerPersonaId="p1"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    )

    const select = (await screen.findByLabelText('执行员工')) as HTMLSelectElement
    expect(select.value).toBe('p2')
    expect(select.disabled).toBe(true)
  })
})
