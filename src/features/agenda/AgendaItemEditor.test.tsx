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
  })

  it('saves new one-shot item', async () => {
    invokeMock.mockResolvedValueOnce({ id: 'agenda-x' })
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
})
