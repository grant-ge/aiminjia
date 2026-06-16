import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { AssistantMarkdown } from '@/components/chat-scene/AssistantMarkdown'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

const tauriMocks = vi.hoisted(() => ({
  getAgendaItem: vi.fn(),
  updateAgendaItem: vi.fn(),
  createAgendaItem: vi.fn(),
  getDefaultFolder: vi.fn(),
  pickLocalDirectory: vi.fn(),
}))

vi.mock('@/hooks/useAuthorizedWorkspace', () => ({
  useAuthorizedWorkspace: () => ({ workspace: null }),
}))

vi.mock('@/lib/tauri', async () => {
  const actual = await vi.importActual<typeof import('@/lib/tauri')>('@/lib/tauri')
  return {
    ...actual,
    getAgendaItem: tauriMocks.getAgendaItem,
    updateAgendaItem: tauriMocks.updateAgendaItem,
    createAgendaItem: tauriMocks.createAgendaItem,
    getDefaultFolder: tauriMocks.getDefaultFolder,
    pickLocalDirectory: tauriMocks.pickLocalDirectory,
  }
})

describe('aijia result cards in assistant markdown', () => {
  beforeEach(() => {
    Object.assign(navigator, {
      clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
    })
    useSkillStore.setState({
      skills: [],
      isLoading: false,
    })
    useUiStore.setState({ route: { kind: 'chat', conversationId: 'conv-1' } })
    tauriMocks.getAgendaItem.mockReset()
    tauriMocks.updateAgendaItem.mockReset()
    tauriMocks.createAgendaItem.mockReset()
    tauriMocks.getDefaultFolder.mockResolvedValue(null)
    tauriMocks.pickLocalDirectory.mockResolvedValue(null)
  })

  it('renders invalid aijia-card JSON as a normal code block', () => {
    render(<AssistantMarkdown text={'```aijia-card\n{ nope\n```'} />)

    expect(screen.getByText('aijia')).toBeInTheDocument()
    expect(screen.getByText('{ nope')).toBeInTheDocument()
  })

  it('renders a skill-created card and opens skill detail', () => {
    useSkillStore.setState({
      skills: [{
        id: 'sales-followup',
        displayName: '销售跟进',
        displayNameEn: 'Sales Follow-up',
        description: '当用户需要创建一个待办任务并进行状态跟踪时使用',
        source: 'custom',
        enabled: true,
        hasWorkflow: true,
        shortDescription: '当用户需要创建一个待办任务并进行状态跟踪时使用',
        shortDescriptionEn: 'Plan next steps',
        triggerText: '/sales-followup',
        category: 'general',
        icon: '',
        updatedAt: null,
      }],
      isLoading: false,
    })

    render(<AssistantMarkdown text={'```aijia-card\n{"type":"skill_created","skillId":"sales-followup"}\n```'} />)

    expect(screen.getByText('销售跟进')).toBeInTheDocument()
    expect(screen.getByText('技能名称')).toBeInTheDocument()
    expect(screen.getByText('触发方式')).toBeInTheDocument()
    expect(screen.getByText('/sales-followup')).toBeInTheDocument()
    expect(screen.getByText('技能描述')).toBeInTheDocument()
    expect(screen.getByText('当用户需要创建一个待办任务并进行状态跟踪时使用')).toBeInTheDocument()
    expect(screen.queryByText('技能已创建')).not.toBeInTheDocument()
    const card = screen.getByText('销售跟进').closest('[data-aijia-result-card="skill_created"]')
    expect(card).toHaveClass('bg-card')
    expect(card).toHaveClass('border-border')
    expect(card).toHaveClass('shadow-none')
    expect(card).toHaveClass('rounded-md')
    expect(card?.querySelector('.lucide-sparkles')).not.toBeInTheDocument()
    expect(card?.querySelector('.lucide-blocks')).not.toBeInTheDocument()
    const viewButton = screen.getByTestId('skill-created-card-view')
    expect(viewButton.parentElement).toBe(card?.firstElementChild)

    fireEvent.click(screen.getByRole('button', { name: '查看技能' }))
    expect(useUiStore.getState().route).toEqual({ kind: 'chat', conversationId: 'conv-1' })
    expect(useUiStore.getState().skillDetailDialogId).toBe('sales-followup')
  })

  it('renders a schedule-created card and opens the editor with live agenda data', async () => {
    tauriMocks.getAgendaItem.mockResolvedValue({
      id: 'agenda-1',
      title: '日报提醒',
      prompt: '每天总结日报',
      organizerEmployeeId: 'default',
      participants: [],
      startAt: '2026-06-13T01:00:00.000Z',
      timezone: 'Asia/Shanghai',
      rule: { freq: 'daily', interval: 1, endCondition: { kind: 'never' } },
      skipDates: [],
      nextFireAt: '2026-06-13T01:00:00.000Z',
      occurrenceCount: 0,
      status: 'active',
      overrideOf: null,
      workspacePath: null,
      createdAt: '2026-06-12T01:00:00.000Z',
      updatedAt: '2026-06-12T01:00:00.000Z',
    })

    render(<AssistantMarkdown text={'```aijia-card\n{"type":"schedule_created","scheduleId":"agenda-1","title":"旧标题"}\n```'} />)

    expect(await screen.findByText('日报提醒')).toBeInTheDocument()
    expect(screen.getByText('日报提醒')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '编辑' }))
    const titleInput = await screen.findByLabelText('标题')
    expect(titleInput).toHaveValue('日报提醒')
  })

  it('prefers live agenda frequency over the stale card snapshot', async () => {
    tauriMocks.getAgendaItem.mockResolvedValue({
      id: 'agenda-1',
      title: '日报提醒',
      prompt: '每天总结日报',
      organizerEmployeeId: 'default',
      participants: [],
      startAt: '2026-06-13T03:00:00.000Z',
      timezone: 'Asia/Shanghai',
      rule: { freq: 'daily', interval: 1, endCondition: { kind: 'never' } },
      skipDates: [],
      nextFireAt: '2026-06-13T03:00:00.000Z',
      occurrenceCount: 0,
      status: 'active',
      overrideOf: null,
      workspacePath: null,
      createdAt: '2026-06-12T01:00:00.000Z',
      updatedAt: '2026-06-12T02:00:00.000Z',
    })

    render(
      <AssistantMarkdown
        text={'```aijia-card\n{"type":"schedule_created","scheduleId":"agenda-1","title":"旧标题","frequencyLabel":"每天 09:00","nextFireAt":"2026-06-13T01:00:00.000Z"}\n```'}
      />,
    )

    expect(await screen.findByText('日报提醒')).toBeInTheDocument()
    expect(screen.getByText(/每天 11:00/)).toBeInTheDocument()
    expect(screen.queryByText('每天 09:00')).not.toBeInTheDocument()
  })

  it('renders the schedule card as a compact white bordered object card', async () => {
    tauriMocks.getAgendaItem.mockResolvedValue({
      id: 'agenda-1',
      title: '日报提醒',
      prompt: '每天总结日报',
      organizerEmployeeId: 'default',
      participants: [],
      startAt: '2026-06-13T01:00:00.000Z',
      timezone: 'Asia/Shanghai',
      rule: { freq: 'daily', interval: 1, endCondition: { kind: 'never' } },
      skipDates: [],
      nextFireAt: '2026-06-13T01:00:00.000Z',
      occurrenceCount: 0,
      status: 'active',
      overrideOf: null,
      workspacePath: null,
      createdAt: '2026-06-12T01:00:00.000Z',
      updatedAt: '2026-06-12T02:00:00.000Z',
    })

    const { container } = render(
      <AssistantMarkdown
        text={'```aijia-card\n{"type":"schedule_created","scheduleId":"agenda-1","title":"旧标题"}\n```'}
      />,
    )

    expect(await screen.findByText('日报提醒')).toBeInTheDocument()
    const card = container.querySelector('[data-aijia-result-card="schedule_created"]')
    expect(card).toHaveClass('bg-card')
    expect(card).toHaveClass('border-border')
    expect(card).toHaveClass('shadow-none')
    expect(card).toHaveClass('rounded-md')
    expect(card?.querySelector('.lucide-calendar-clock')).not.toBeInTheDocument()
    const editButton = screen.getByTestId('schedule-created-card-edit')
    expect(editButton.parentElement).toBe(card?.firstElementChild)
    expect(screen.queryByText('定时任务已创建')).not.toBeInTheDocument()
    expect(screen.queryByText('agenda-1')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '查看定时任务' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: '编辑' })).toBeInTheDocument()
    expect(screen.getByText('频率')).toBeInTheDocument()
    expect(screen.getByText('下次运行')).toBeInTheDocument()
  })

  it('edits a schedule through agenda APIs without mutating assistant markdown', async () => {
    const originalText = '```aijia-card\n{"type":"schedule_created","scheduleId":"agenda-1","title":"旧标题"}\n```'
    tauriMocks.getAgendaItem.mockResolvedValue({
      id: 'agenda-1',
      title: '日报提醒',
      prompt: '每天总结日报',
      organizerEmployeeId: 'default',
      participants: [],
      startAt: '2026-06-13T01:00:00.000Z',
      timezone: 'Asia/Shanghai',
      rule: null,
      skipDates: [],
      nextFireAt: null,
      occurrenceCount: 0,
      status: 'active',
      overrideOf: null,
      workspacePath: null,
      createdAt: '2026-06-12T01:00:00.000Z',
      updatedAt: '2026-06-12T01:00:00.000Z',
    })
    tauriMocks.updateAgendaItem.mockResolvedValue({
      id: 'agenda-1',
      title: '日报提醒新版',
      prompt: '每天总结日报',
      organizerEmployeeId: 'default',
      participants: [],
      startAt: '2026-06-13T01:00:00.000Z',
      timezone: 'Asia/Shanghai',
      rule: null,
      skipDates: [],
      nextFireAt: null,
      occurrenceCount: 0,
      status: 'active',
      overrideOf: null,
      workspacePath: null,
      createdAt: '2026-06-12T01:00:00.000Z',
      updatedAt: '2026-06-12T02:00:00.000Z',
    })

    const { rerender } = render(<AssistantMarkdown text={originalText} />)
    fireEvent.click(await screen.findByRole('button', { name: '编辑' }))
    fireEvent.change(await screen.findByLabelText('标题'), {
      target: { value: '日报提醒新版' },
    })
    fireEvent.click(screen.getByRole('button', { name: '保存' }))

    await waitFor(() => {
      expect(tauriMocks.updateAgendaItem).toHaveBeenCalledWith('agenda-1', expect.objectContaining({
        title: '日报提醒新版',
      }))
    })

    rerender(<AssistantMarkdown text={originalText} />)
    expect(originalText).toContain('"title":"旧标题"')
  })
})
