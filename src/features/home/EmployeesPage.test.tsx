import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  refreshEmployees: vi.fn(async () => undefined),
  refreshInbox: vi.fn(async () => undefined),
  markRead: vi.fn(async () => undefined),
  employeeTemplateRefresh: vi.fn(async () => 0),
  pushNotification: vi.fn(),
  setRoute: vi.fn(),
  setSidebarTab: vi.fn(),
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
  employees: [] as Array<{
    id: string
    name: string
    avatar: string
    role: string
    description: string
    lifecycle: string
  }>,
}))

vi.mock('@/features/employees/useEmployees', () => ({
  useEmployees: () => ({
    employees: mocks.employees,
    activeRuns: {},
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

vi.mock('@/features/employees/EmployeeDrawer', () => ({
  EmployeeDrawer: () => null,
}))

vi.mock('@/features/employees/HireWizard', () => ({
  HireWizard: () => null,
}))

vi.mock('@/lib/tauri', () => ({
  employeeTemplateRefresh: mocks.employeeTemplateRefresh,
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

import { EmployeesPage } from './EmployeesPage'

describe('EmployeesPage', () => {
  beforeEach(() => {
    mocks.refreshEmployees.mockClear()
    mocks.refreshInbox.mockClear()
    mocks.markRead.mockClear()
    mocks.employeeTemplateRefresh.mockClear()
    mocks.pushNotification.mockClear()
    mocks.setRoute.mockClear()
    mocks.setSidebarTab.mockClear()
    mocks.entries = []
    mocks.employees = []
  })

  it('automatically syncs employee templates once when the page opens', async () => {
    render(<EmployeesPage />)

    await waitFor(() => {
      expect(mocks.employeeTemplateRefresh).toHaveBeenCalledTimes(1)
    })
    expect(mocks.refreshEmployees).toHaveBeenCalled()
  })

  it('opens the employee market from the employees page without exposing page-level sync', async () => {
    render(<EmployeesPage />)
    await waitFor(() => {
      expect(mocks.employeeTemplateRefresh).toHaveBeenCalledTimes(1)
    })

    expect(screen.queryByRole('button', { name: '同步服务端' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: '员工市场' })).toBeInTheDocument()
  })

  it('switches to the employee sidebar tab before opening a today feed conversation', () => {
    mocks.employees = [{
      id: 'emp-1',
      name: '小程',
      avatar: '🛠️',
      role: '流程设计师',
      description: '流程设计师',
      lifecycle: 'active',
    }]
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

    fireEvent.click(screen.getByRole('button', { name: /小程 已完成任务/ }))

    expect(mocks.markRead).toHaveBeenCalledWith('emp-1', 'entry-1')
    expect(mocks.setSidebarTab).toHaveBeenCalledWith('employee')
    expect(mocks.setRoute).toHaveBeenCalledWith({ kind: 'chat', conversationId: 'conv-1' })
    expect(mocks.setSidebarTab.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.setRoute.mock.invocationCallOrder[0],
    )
  })
})
