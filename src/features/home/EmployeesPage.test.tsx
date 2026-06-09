import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '@/i18n'
import type {
  EmployeeRecord,
  EmployeeTemplateSnapshot,
  WorkplaceDirectoryResponse,
} from '@/lib/tauri'

const mocks = vi.hoisted(() => ({
  refreshEmployees: vi.fn(async () => undefined),
  refreshInbox: vi.fn(async () => undefined),
  markRead: vi.fn(async () => undefined),
  employeeTemplateCatalog: vi.fn<() => Promise<EmployeeTemplateSnapshot[]>>(async () => []),
  employeeTemplateRefresh: vi.fn(async () => 0),
  workplaceDirectoryCatalog: vi.fn<(
    lang?: string,
    options?: { forceRefresh?: boolean },
  ) => Promise<WorkplaceDirectoryResponse>>(
    async () => ({ schemaVersion: 1, categories: [], items: [] }),
  ),
  employeeCreate: vi.fn(),
  employeeTrigger: vi.fn(async () => 'conv-created'),
  pushNotification: vi.fn(),
  setRoute: vi.fn(),
  setSidebarTab: vi.fn(),
  setConversations: vi.fn(),
  setMessages: vi.fn(),
  chatConversations: [] as unknown[],
  activeRuns: {} as Record<string, {
    employeeId: string
    conversationId: string
    startedAt: string
    triggerKind: 'on_demand' | 'cron'
  }>,
  entries: [] as Array<{
    id: string
    employeeId: string
    kind: 'report' | 'signal' | 'running' | 'error'
    title: string
    summary: string | null
    reportPath: string | null
    conversationId: string | null
    read: boolean
    catchupInfo: string | null
    createdAt: string
  }>,
  employees: [] as EmployeeRecord[],
}))

vi.mock('@/features/employees/useEmployees', () => ({
  useEmployees: () => ({
    employees: mocks.employees,
    activeRuns: mocks.activeRuns,
    loading: false,
    refresh: mocks.refreshEmployees,
  }),
}))

vi.mock('@/features/employees/useInbox', () => ({
  useInbox: () => ({
    entries: mocks.entries,
    refresh: mocks.refreshInbox,
    markRead: mocks.markRead,
  }),
}))

vi.mock('@/lib/tauri', () => ({
  employeeTemplateCatalog: mocks.employeeTemplateCatalog,
  employeeTemplateRefresh: mocks.employeeTemplateRefresh,
  workplaceDirectoryCatalog: mocks.workplaceDirectoryCatalog,
  employeeCreate: mocks.employeeCreate,
  employeeTrigger: mocks.employeeTrigger,
}))

vi.mock('@/stores/uiStore', () => ({
  useUiStore: (selector: (state: unknown) => unknown) => selector({
    setRoute: mocks.setRoute,
    setSidebarTab: mocks.setSidebarTab,
  }),
}))

vi.mock('@/stores/authStore', () => ({
  useAuthStore: (selector: (state: unknown) => unknown) => selector({ isLoggedIn: true }),
}))

vi.mock('@/stores/notificationStore', () => ({
  useNotificationStore: (selector: (state: unknown) => unknown) => selector({ push: mocks.pushNotification }),
}))

vi.mock('@/stores/chatStore', () => ({
  useChatStore: {
    getState: () => ({
      conversations: mocks.chatConversations,
      setConversations: mocks.setConversations,
      setMessages: mocks.setMessages,
    }),
  },
}))

import { EmployeesPage } from './EmployeesPage'

function makeSnapshot(overrides: Partial<EmployeeTemplateSnapshot> = {}): EmployeeTemplateSnapshot {
  return {
    templateId: 'builtin:xiaocheng',
    version: '1.0.0',
    name: '程砚舟',
    avatar: '🛠️',
    role: '流程设计师',
    description: '通过对话拆解你的工作流程。',
    badge: '开箱即用',
    systemPromptExtra: '你是流程设计师。',
    toolWhitelist: ['Skill'],
    cron: '',
    defaultSkillId: '',
    requiresDingtalk: false,
    requiresAttachment: null,
    resourceConfigSchema: null,
    resourceConfigUI: null,
    ...overrides,
  }
}

function makeDirectory(): WorkplaceDirectoryResponse {
  return {
    schemaVersion: 1,
    categories: [{
      categoryId: 'delivery',
      display: { name: '研发交付', description: '把流程和交付事项同步下来。' },
      icon: '🛠️',
      color: '#2563eb',
      sortOrder: 10,
      resourceCount: 1,
    }],
    items: [{
      resourceType: 'employee_template',
      resourceId: 'builtin:xiaocheng',
      version: '1.0.0',
      workplaceCategoryId: 'delivery',
      sortOrder: 10,
      display: { name: '小程', description: '通过对话拆解你的工作流程。' },
      icon: '🛠️',
      requiredSkills: [{
        skillId: 'workflow-design',
        source: 'platform',
        scope: 'public',
        display: { name: '流程设计' },
        versionRange: '',
      }],
    }],
  }
}

function makeEmployee(overrides: Partial<EmployeeRecord> = {}): EmployeeRecord {
  const now = '2026-06-05T12:00:00Z'
  return {
    id: 'emp-created',
    name: '程砚舟',
    role: '流程设计师',
    description: '通过对话拆解你的工作流程。',
    avatar: '🛠️',
    templateId: 'builtin:xiaocheng',
    toolWhitelist: ['Skill'],
    cron: null,
    timezone: 'Asia/Shanghai',
    lifecycle: 'active',
    cronEnabled: false,
    resourceConfig: {},
    systemPromptExtra: '你是流程设计师。',
    defaultSkillId: null,
    templateRef: null,
    createdAt: now,
    updatedAt: now,
    lastRunAt: null,
    nextRunAt: null,
    ...overrides,
  }
}

describe('EmployeesPage', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('zh-CN')
    mocks.refreshEmployees.mockClear()
    mocks.refreshInbox.mockClear()
    mocks.markRead.mockClear()
    mocks.employeeTemplateCatalog.mockReset()
    mocks.employeeTemplateCatalog.mockResolvedValue([makeSnapshot()])
    mocks.employeeTemplateRefresh.mockReset()
    mocks.employeeTemplateRefresh.mockResolvedValue(0)
    mocks.workplaceDirectoryCatalog.mockReset()
    mocks.workplaceDirectoryCatalog.mockResolvedValue(makeDirectory())
    mocks.employeeCreate.mockReset()
    mocks.employeeCreate.mockResolvedValue(makeEmployee())
    mocks.employeeTrigger.mockReset()
    mocks.employeeTrigger.mockResolvedValue('conv-created')
    mocks.pushNotification.mockClear()
    mocks.setRoute.mockClear()
    mocks.setSidebarTab.mockClear()
    mocks.setConversations.mockReset()
    mocks.setConversations.mockImplementation((next: unknown[]) => {
      mocks.chatConversations = next
    })
    mocks.setMessages.mockClear()
    mocks.chatConversations = []
    mocks.activeRuns = {}
    mocks.entries = []
    mocks.employees = []
  })

  it('automatically syncs the workplace employee directory when the page opens', async () => {
    render(<EmployeesPage />)

    await waitFor(() => {
      expect(mocks.workplaceDirectoryCatalog).toHaveBeenCalled()
    })
    expect(mocks.workplaceDirectoryCatalog).not.toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ forceRefresh: true }),
    )
    expect(mocks.employeeTemplateCatalog).toHaveBeenCalled()
    expect(mocks.employeeTemplateRefresh).not.toHaveBeenCalled()
    expect(mocks.refreshEmployees).toHaveBeenCalled()
  })

  it('forces a server refresh when users click update content', async () => {
    render(<EmployeesPage />)

    await waitFor(() => {
      expect(mocks.workplaceDirectoryCatalog).toHaveBeenCalled()
    })
    mocks.workplaceDirectoryCatalog.mockClear()

    fireEvent.click(screen.getByRole('button', { name: '更新内容' }))

    await waitFor(() => {
      expect(mocks.workplaceDirectoryCatalog).toHaveBeenCalledWith(
        expect.any(String),
        { forceRefresh: true },
      )
    })
  })

  it('renders server-side employee categories directly on the page', async () => {
    render(<EmployeesPage />)

    expect(await screen.findByText('研发交付')).toBeInTheDocument()
    expect(screen.getByText('程砚舟')).toBeInTheDocument()
    expect(screen.getByText('流程设计')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '员工市场' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: '更新内容' })).toBeInTheDocument()
  })

  it('filters employee templates with horizontal category tabs', async () => {
    mocks.workplaceDirectoryCatalog.mockResolvedValueOnce({
      schemaVersion: 1,
      categories: [
        {
          categoryId: 'delivery',
          display: { name: '研发交付', description: '把流程和交付事项同步下来。' },
          icon: '🛠️',
          color: '#2563eb',
          sortOrder: 10,
          resourceCount: 1,
        },
        {
          categoryId: 'legal',
          display: { name: '法务合规', description: '合同和合规事项。' },
          icon: '⚖️',
          color: '#7c3aed',
          sortOrder: 20,
          resourceCount: 1,
        },
      ],
      items: [
        {
          resourceType: 'employee_template',
          resourceId: 'builtin:xiaocheng',
          version: '1.0.0',
          workplaceCategoryId: 'delivery',
          sortOrder: 10,
          display: { name: '小程', description: '通过对话拆解你的工作流程。' },
          icon: '🛠️',
          requiredSkills: [],
        },
        {
          resourceType: 'employee_template',
          resourceId: 'builtin:xiaofa',
          version: '1.0.0',
          workplaceCategoryId: 'legal',
          sortOrder: 10,
          display: { name: '小法', description: '审阅合同风险。' },
          icon: '⚖️',
          requiredSkills: [],
        },
      ],
    })
    mocks.employeeTemplateCatalog.mockResolvedValueOnce([
      makeSnapshot(),
      makeSnapshot({
        templateId: 'builtin:xiaofa',
        name: '陈景律',
        avatar: '⚖️',
        role: '合同与合规顾问',
        description: '审阅合同风险。',
      }),
    ])

    render(<EmployeesPage />)

    expect(await screen.findByRole('button', { name: '全部' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '研发交付' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '法务合规' })).toBeInTheDocument()
    expect(screen.getByText('程砚舟')).toBeInTheDocument()
    expect(screen.getByText('陈景律')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '法务合规' }))

    expect(screen.queryByText('程砚舟')).not.toBeInTheDocument()
    expect(screen.getByText('陈景律')).toBeInTheDocument()
    expect(screen.getByText('合同和合规事项。')).toBeInTheDocument()
  })

  it('omits placeholder labels for uncategorized employee templates', async () => {
    mocks.workplaceDirectoryCatalog.mockResolvedValueOnce({ schemaVersion: 1, categories: [], items: [] })

    render(<EmployeesPage />)

    expect(await screen.findByText('程砚舟')).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: '数字员工' })).not.toBeInTheDocument()
    expect(screen.queryByText('其他员工')).not.toBeInTheDocument()
  })

  it('only reveals the employee template detail action on card hover or focus', async () => {
    render(<EmployeesPage />)

    const action = await screen.findByText('查看详情')
    expect(action).toHaveClass('opacity-0')
    expect(action).toHaveClass('group-hover:opacity-100')
    expect(action).toHaveClass('group-focus-visible:opacity-100')
  })

  it('renders employee template cards in a four-column desktop grid', async () => {
    render(<EmployeesPage />)

    const card = await screen.findByRole('button', { name: '查看 程砚舟 详情' })
    expect(card).toHaveClass('w-full')
    expect(card).toHaveClass('h-[154px]')
    expect(card.closest('.grid')).toHaveClass('xl:grid-cols-4')
    expect(card.querySelector('.h-9.w-9')).toBeInTheDocument()
    expect(card.querySelector('.line-clamp-2')).toHaveClass('mt-1')
  })

  it('strips emoji from employee template chips', async () => {
    const directory = makeDirectory()
    directory.items[0] = {
      ...directory.items[0],
      requiredSkills: [{
        skillId: 'workflow-design',
        source: 'platform',
        scope: 'public',
        display: { name: '⚙️ 自动巡检' },
        versionRange: '',
      }],
    }
    mocks.workplaceDirectoryCatalog.mockResolvedValueOnce(directory)
    mocks.employeeTemplateCatalog.mockResolvedValueOnce([
      makeSnapshot({ badge: '🟢 开箱即用' }),
    ])

    render(<EmployeesPage />)

    expect(await screen.findByText('开箱即用')).toBeInTheDocument()
    expect(screen.getByText('自动巡检')).toBeInTheDocument()
    expect(screen.queryByText('🟢 开箱即用')).not.toBeInTheDocument()
    expect(screen.queryByText('⚙️ 自动巡检')).not.toBeInTheDocument()
  })

  it('renders employee template chips with a unified muted style', async () => {
    render(<EmployeesPage />)

    const chips = [
      await screen.findByText('开箱即用'),
      screen.getByText('v1.0.0'),
      screen.getByText('流程设计'),
    ]

    for (const chip of chips) {
      expect(chip).toHaveClass('rounded-md')
      expect(chip).toHaveClass('bg-muted')
      expect(chip).toHaveClass('text-muted-foreground')
      expect(chip).not.toHaveClass('bg-accent')
      expect(chip).not.toHaveClass('bg-secondary')
      expect(chip).not.toHaveClass('text-accent-foreground')
    }
  })

  it('opens employee details first, then creates and dispatches from the detail action', async () => {
    render(<EmployeesPage />)

    fireEvent.click(await screen.findByRole('button', { name: '查看 程砚舟 详情' }))
    expect(screen.getByText('适合交给 TA 的任务')).toBeInTheDocument()
    expect(mocks.employeeCreate).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: '召唤' }))

    await waitFor(() => {
      expect(mocks.employeeCreate).toHaveBeenCalledWith(expect.objectContaining({
        templateId: 'builtin:xiaocheng',
        name: '程砚舟',
        cronEnabled: false,
        resourceConfig: {},
      }))
    })
    expect(mocks.employeeTrigger).toHaveBeenCalledWith('emp-created', undefined, [])
    expect(mocks.setConversations).toHaveBeenCalledWith([
      expect.objectContaining({
        id: 'conv-created',
        title: '派活: 程砚舟',
        kind: 'employee',
        sourceLabel: '程砚舟',
      }),
    ])
    expect(mocks.setMessages).toHaveBeenCalledWith([])
    expect(mocks.setSidebarTab).toHaveBeenCalledWith('employee')
    expect(mocks.setRoute).toHaveBeenCalledWith({ kind: 'chat', conversationId: 'conv-created' })
  })

  it('reuses an existing employee instead of creating a duplicate', async () => {
    mocks.employees = [makeEmployee({ id: 'emp-existing' })]
    render(<EmployeesPage />)

    fireEvent.click(await screen.findByRole('button', { name: '查看 程砚舟 详情' }))
    fireEvent.click(screen.getByRole('button', { name: '派活' }))

    await waitFor(() => {
      expect(mocks.employeeTrigger).toHaveBeenCalledWith('emp-existing', undefined, [])
    })
    expect(mocks.employeeCreate).not.toHaveBeenCalled()
  })

  it('opens the active run conversation instead of dispatching again', async () => {
    mocks.employees = [makeEmployee({ id: 'emp-existing' })]
    mocks.activeRuns = {
      'emp-existing': {
        employeeId: 'emp-existing',
        conversationId: 'conv-running',
        startedAt: '2026-06-05T12:00:00Z',
        triggerKind: 'on_demand',
      },
    }
    render(<EmployeesPage />)

    fireEvent.click(await screen.findByRole('button', { name: '查看 程砚舟 详情' }))
    fireEvent.click(screen.getByRole('button', { name: '进入会话' }))

    await waitFor(() => {
      expect(mocks.setRoute).toHaveBeenCalledWith({ kind: 'chat', conversationId: 'conv-running' })
    })
    expect(mocks.employeeCreate).not.toHaveBeenCalled()
    expect(mocks.employeeTrigger).not.toHaveBeenCalled()
  })

  it('switches to the employee sidebar tab before opening a today feed conversation', async () => {
    mocks.employees = [makeEmployee({ id: 'emp-1' })]
    mocks.entries = [{
      id: 'entry-1',
      employeeId: 'emp-1',
      kind: 'report',
      title: '小程 已完成任务',
      summary: '我是小程，流程设计师。',
      reportPath: null,
      conversationId: 'conv-1',
      read: false,
      catchupInfo: null,
      createdAt: new Date().toISOString(),
    }]

    render(<EmployeesPage />)

    await screen.findByText('研发交付')
    fireEvent.click(screen.getByRole('button', { name: /小程 已完成任务/ }))

    expect(mocks.markRead).toHaveBeenCalledWith('emp-1', 'entry-1')
    expect(mocks.setSidebarTab).toHaveBeenCalledWith('employee')
    expect(mocks.setRoute).toHaveBeenCalledWith({ kind: 'chat', conversationId: 'conv-1' })
    expect(mocks.setSidebarTab.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.setRoute.mock.invocationCallOrder[0],
    )
  })
})
