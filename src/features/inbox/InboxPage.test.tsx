import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  markAllRead: vi.fn(async () => undefined),
  markRead: vi.fn(async () => undefined),
  setRoute: vi.fn(),
  setSidebarTab: vi.fn(),
  employees: [{
    id: 'emp-1',
    name: '小程',
    avatar: '🛠️',
    lifecycle: 'active',
  }],
  entries: [{
    id: 'entry-1',
    employeeId: 'emp-1',
    kind: 'report' as const,
    title: '小程 已完成任务',
    summary: '我是小程，流程设计师。',
    reportPath: null,
    conversationId: 'conv-1',
    read: false,
    catchupInfo: null,
    createdAt: new Date().toISOString(),
  }],
}))

vi.mock('@/features/employees/useEmployees', () => ({
  useEmployees: () => ({
    employees: mocks.employees,
  }),
}))

vi.mock('@/features/employees/useInbox', () => ({
  useInbox: () => ({
    entries: mocks.entries,
    markAllRead: mocks.markAllRead,
    markRead: mocks.markRead,
  }),
}))

vi.mock('@/stores/uiStore', () => ({
  useUiStore: (selector: (state: unknown) => unknown) => selector({
    setRoute: mocks.setRoute,
    setSidebarTab: mocks.setSidebarTab,
  }),
}))

import { InboxPage } from './InboxPage'

describe('InboxPage', () => {
  beforeEach(() => {
    mocks.markAllRead.mockClear()
    mocks.markRead.mockClear()
    mocks.setRoute.mockClear()
    mocks.setSidebarTab.mockClear()
  })

  it('switches to the employee sidebar tab before opening an inbox conversation', () => {
    render(<InboxPage />)

    fireEvent.click(screen.getByRole('button', { name: /小程 已完成任务/ }))

    expect(mocks.markRead).toHaveBeenCalledWith('emp-1', 'entry-1')
    expect(mocks.setSidebarTab).toHaveBeenCalledWith('employee')
    expect(mocks.setRoute).toHaveBeenCalledWith({ kind: 'chat', conversationId: 'conv-1' })
    expect(mocks.setSidebarTab.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.setRoute.mock.invocationCallOrder[0],
    )
  })
})
