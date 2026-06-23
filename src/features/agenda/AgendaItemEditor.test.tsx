import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'

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

function pickStartMinute(hour: number, minute: number) {
  fireEvent.click(screen.getByRole('button', { name: '开始时间' }))
  fireEvent.click(screen.getByRole('button', { name: `选择小时 ${String(hour).padStart(2, '0')}` }))
  fireEvent.click(screen.getByRole('button', { name: `选择分钟 ${String(minute).padStart(2, '0')}` }))
  fireEvent.click(screen.getByRole('button', { name: '确定' }))
}

function chooseSelectOption(label: string, option: string) {
  fireEvent.pointerDown(screen.getByRole('combobox', { name: label }), {
    button: 0,
    ctrlKey: false,
    pointerType: 'mouse',
  })
  fireEvent.click(within(screen.getByRole('listbox')).getByRole('option', { name: option }))
}

describe('AgendaItemEditor', () => {
  beforeAll(() => {
    Object.defineProperty(HTMLElement.prototype, 'hasPointerCapture', {
      configurable: true,
      value: () => false,
    })
    Object.defineProperty(HTMLElement.prototype, 'setPointerCapture', {
      configurable: true,
      value: () => undefined,
    })
    Object.defineProperty(HTMLElement.prototype, 'releasePointerCapture', {
      configurable: true,
      value: () => undefined,
    })
  })

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

  afterEach(() => {
    vi.useRealTimers()
  })

  it('shows a default start time when creating a new item', () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(2026, 4, 7, 13, 24, 30))

    render(
      <AgendaItemEditor
        open
        organizerEmployeeId="emp-1"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    )

    expect(screen.getByText('2026年5月7日 13:25')).toBeInTheDocument()
  })

  it('renders title and prompt as labeled form fields', () => {
    render(
      <AgendaItemEditor
        open
        organizerEmployeeId="emp-1"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    )

    expect(screen.getByLabelText('标题')).toBeInstanceOf(HTMLInputElement)
    expect(screen.getByLabelText('到点要做什么？')).toBeInstanceOf(HTMLTextAreaElement)
    expect(screen.getByText('标题')).toHaveClass('text-sm')
    expect(screen.getByText('到点要做什么？')).toHaveClass('text-sm')
  })

  it('keeps the sheet shell fixed while only the form content scrolls', () => {
    render(
      <AgendaItemEditor
        open
        organizerEmployeeId="emp-1"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    )

    const sheet = document.querySelector('[data-aijia-agenda-editor]')
    const header = document.querySelector('[data-aijia-agenda-header]')
    const body = document.querySelector('[data-aijia-agenda-form-body]')
    const footer = document.querySelector('[data-aijia-agenda-footer]')

    expect(sheet).toHaveClass('p-0')
    expect(sheet).not.toHaveClass('p-6')
    expect(header).toHaveClass('shrink-0')
    expect(header).toHaveClass('h-[3.5rem]')
    expect(header).toHaveClass('justify-center')
    expect(header).not.toHaveClass('overflow-y-auto')
    expect(screen.getByRole('heading', { name: '新建日程' })).toHaveClass('text-md')
    expect(body).toHaveClass('min-h-0')
    expect(body).toHaveClass('flex-1')
    expect(body).toHaveClass('overflow-y-auto')
    expect(body).toHaveClass('px-6')
    expect(body).toHaveClass('py-5')
    expect(footer).toHaveClass('shrink-0')
    expect(footer).toHaveClass('h-[4.0625rem]')
    expect(footer).toHaveClass('justify-end')
    expect(footer).not.toHaveClass('overflow-y-auto')
    expect(footer).toHaveClass('px-6')
    expect(footer).toHaveClass('py-0')
  })

  it('exposes visible agenda controls for intent-test commands without hidden compat controls', () => {
    render(
      <AgendaItemEditor
        open
        organizerEmployeeId="emp-1"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    )

    expect(document.querySelector('[data-aijia-agenda-compat]')).not.toBeInTheDocument()
    expect(document.querySelector('[data-aijia-agenda-field="frequency"]')).toBe(
      screen.getByRole('combobox', { name: '频率' }),
    )
    expect(screen.getByRole('button', { name: '开始时间' })).toHaveAttribute(
      'data-aijia-date-time-trigger',
      'agenda-editor-start',
    )
  })

  it('saves with the default start time when users do not manually pick one', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(2026, 4, 7, 13, 24, 30))
    const onSaved = vi.fn()
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_default_folder') return null
      if (cmd === 'create_agenda_item') return { id: 'agenda-x' }
      return null
    })

    render(
      <AgendaItemEditor
        open
        organizerEmployeeId="emp-1"
        onClose={() => {}}
        onSaved={onSaved}
      />,
    )

    fireEvent.change(screen.getByLabelText('标题'), { target: { value: 'T' } })
    fireEvent.change(screen.getByLabelText('到点要做什么？'), { target: { value: 'P' } })
    vi.useRealTimers()
    fireEvent.click(screen.getByRole('button', { name: '保存' }))

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        'create_agenda_item',
        expect.objectContaining({
          request: expect.objectContaining({
            startAt: new Date('2026-05-07T13:25').toISOString(),
          }),
        }),
      )
      expect(onSaved).toHaveBeenCalled()
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

    fireEvent.change(screen.getByLabelText('标题'), { target: { value: 'T' } })
    fireEvent.change(screen.getByLabelText('到点要做什么？'), { target: { value: 'P' } })
    pickStartMinute(9, 0)
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
    chooseSelectOption('频率', '每天')
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
    fireEvent.change(screen.getByLabelText('标题'), { target: { value: 'T' } })
    fireEvent.change(screen.getByLabelText('到点要做什么？'), { target: { value: 'P' } })
    pickStartMinute(9, 0)
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
    fireEvent.change(screen.getByLabelText('标题'), { target: { value: 'T' } })
    fireEvent.change(screen.getByLabelText('到点要做什么？'), { target: { value: 'P' } })
    pickStartMinute(9, 0)
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

  it('lets users pick an exact start hour and minute from the editor', async () => {
    const itemStartAt = '2026-05-07T01:00:00.000Z'
    const item = {
      id: 'a1',
      title: '巡检汇总',
      prompt: '生成报表',
      startAt: itemStartAt,
      timezone: 'Asia/Shanghai',
      organizerEmployeeId: 'emp-2',
      rule: null,
      workspacePath: null,
    } as unknown as Parameters<typeof AgendaItemEditor>[0]['initial']
    const expectedStartAt = new Date(itemStartAt)
    expectedStartAt.setHours(10, 45, 0, 0)

    render(
      <AgendaItemEditor
        open
        initial={item}
        organizerEmployeeId="emp-1"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    )

    pickStartMinute(10, 45)
    fireEvent.click(screen.getByRole('button', { name: '保存' }))

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        'update_agenda_item',
        expect.objectContaining({
          id: 'a1',
          request: expect.objectContaining({
            startAt: expectedStartAt.toISOString(),
          }),
        }),
      )
    })
  })
})
