import '@testing-library/jest-dom'
import { render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  refreshEmployees: vi.fn(async () => undefined),
  refreshInbox: vi.fn(async () => undefined),
  employeeTemplateRefresh: vi.fn(async () => 0),
  pushNotification: vi.fn(),
}))

vi.mock('@/features/employees/useEmployees', () => ({
  useEmployees: () => ({
    employees: [],
    activeRuns: {},
    loading: false,
    refresh: mocks.refreshEmployees,
  }),
}))

vi.mock('@/features/employees/useInbox', () => ({
  useInbox: () => ({
    entries: [],
    refresh: mocks.refreshInbox,
    markRead: vi.fn(),
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
  useUiStore: (selector: (state: unknown) => unknown) => selector({ setRoute: vi.fn() }),
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
    mocks.employeeTemplateRefresh.mockClear()
    mocks.pushNotification.mockClear()
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
})
