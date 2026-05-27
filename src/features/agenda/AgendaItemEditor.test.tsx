import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const invokeMock = vi.hoisted(() => vi.fn())
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

import { AgendaItemEditor } from './AgendaItemEditor'

const TWO_EMPLOYEES = [
  {
    id: 'emp-1',
    name: '小研',
    avatar: '🔍',
    role: '调研',
    description: '',
    templateId: 'builtin:xiaoyuan',
    toolWhitelist: [],
    cron: null,
    timezone: 'Asia/Shanghai',
    lifecycle: 'active',
    cronEnabled: true,
    resourceConfig: {},
    systemPromptExtra: null,
    defaultSkillId: null,
    createdAt: '2026-05-09T00:00:00Z',
    updatedAt: '2026-05-09T00:00:00Z',
    lastRunAt: null,
    nextRunAt: null,
  },
  {
    id: 'emp-2',
    name: '小法',
    avatar: '⚖️',
    role: '合同',
    description: '',
    templateId: 'builtin:xiaofa',
    toolWhitelist: [],
    cron: null,
    timezone: 'Asia/Shanghai',
    lifecycle: 'active',
    cronEnabled: true,
    resourceConfig: {},
    systemPromptExtra: null,
    defaultSkillId: null,
    createdAt: '2026-05-09T00:00:00Z',
    updatedAt: '2026-05-09T00:00:00Z',
    lastRunAt: null,
    nextRunAt: null,
  },
]

describe('AgendaItemEditor', () => {
  beforeEach(() => {
    invokeMock.mockReset()
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_default_folder') {
        return { id: 'default', rootPath: '/tmp/default', displayName: 'default' }
      }
      if (cmd === 'employee_list') {
        return TWO_EMPLOYEES
      }
      return null
    })
  })

  it('saves new one-shot item', async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_default_folder') {
        return { id: 'default', rootPath: '/tmp/default', displayName: 'default' }
      }
      if (cmd === 'employee_list') return [TWO_EMPLOYEES[0]]
      if (cmd === 'create_agenda_item') return { id: 'agenda-x' }
      return null
    })
    const onSaved = vi.fn()

    render(
      <AgendaItemEditor
        open
        organizerEmployeeId="emp-1"
        onClose={() => {}}
        onSaved={onSaved}
      />,
    )

    fireEvent.change(screen.getByPlaceholderText('标题'), { target: { value: 'T' } })
    fireEvent.change(screen.getByPlaceholderText('到点要做什么？'), { target: { value: 'P' } })
    fireEvent.change(screen.getByLabelText(/开始时间/), { target: { value: '2026-05-07T09:00' } })
    fireEvent.click(screen.getByRole('button', { name: '保存' }))

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        'create_agenda_item',
        expect.objectContaining({
          request: expect.objectContaining({
            title: 'T',
            prompt: 'P',
            organizerEmployeeId: 'emp-1',
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
        organizerEmployeeId="emp-1"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    )
    expect(screen.queryByLabelText('结束条件')).toBeNull()
    fireEvent.change(screen.getByLabelText('频率'), { target: { value: 'daily' } })
    await waitFor(() => expect(screen.getByLabelText('结束条件')).toBeInTheDocument())
  })

  it('does not render an employee selector when creating', async () => {
    render(
      <AgendaItemEditor
        open
        organizerEmployeeId="emp-2"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    )
    expect(screen.queryByLabelText('执行员工')).not.toBeInTheDocument()
  })

  it('uses the default organizer id when creating', async () => {
    const onSaved = vi.fn()
    render(
      <AgendaItemEditor
        open
        organizerEmployeeId="emp-1"
        onClose={() => {}}
        onSaved={onSaved}
      />,
    )
    fireEvent.change(screen.getByPlaceholderText('标题'), { target: { value: 'T' } })
    fireEvent.change(screen.getByPlaceholderText('到点要做什么？'), { target: { value: 'P' } })
    fireEvent.change(screen.getByLabelText(/开始时间/), { target: { value: '2026-05-07T09:00' } })
    fireEvent.click(screen.getByRole('button', { name: '保存' }))
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        'create_agenda_item',
        expect.objectContaining({
          request: expect.objectContaining({ organizerEmployeeId: 'emp-1' }),
        }),
      )
      expect(onSaved).toHaveBeenCalled()
    })
  })

  it('allows creating an agenda item without any hired employee', async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_default_folder') return null
      if (cmd === 'create_agenda_item') return { id: 'agenda-x' }
      return null
    })
    const onSaved = vi.fn()
    render(
      <AgendaItemEditor
        open
        organizerEmployeeId=""
        onClose={() => {}}
        onSaved={onSaved}
      />,
    )
    fireEvent.change(screen.getByPlaceholderText('标题'), { target: { value: 'T' } })
    fireEvent.change(screen.getByPlaceholderText('到点要做什么？'), { target: { value: 'P' } })
    fireEvent.change(screen.getByLabelText(/开始时间/), { target: { value: '2026-05-07T09:00' } })
    fireEvent.click(screen.getByRole('button', { name: '保存' }))

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        'create_agenda_item',
        expect.objectContaining({
          request: expect.objectContaining({ organizerEmployeeId: 'default' }),
        }),
      )
      expect(onSaved).toHaveBeenCalled()
    })
  })

  it('keeps the existing organizer id when editing an item', async () => {
    const item = {
      id: 'a1',
      title: 'X',
      prompt: 'Y',
      startAt: '2026-05-07T01:00:00.000Z',
      timezone: 'Asia/Shanghai',
      organizerEmployeeId: 'emp-2',
      rule: null,
      workspacePath: null,
    } as unknown as Parameters<typeof AgendaItemEditor>[0]['initial']

    render(
      <AgendaItemEditor
        open
        initial={item}
        organizerEmployeeId="emp-1"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    )
    expect(screen.queryByLabelText('执行员工')).not.toBeInTheDocument()
  })
})
