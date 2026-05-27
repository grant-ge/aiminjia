import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '@/i18n'

const mocks = vi.hoisted(() => ({
  createConversation: vi.fn(),
  renameConversation: vi.fn(),
  expertTeamTemplateRefresh: vi.fn(async () => 0),
  pushNotification: vi.fn(),
}))

vi.mock('@/lib/tauri', () => ({
  createConversation: mocks.createConversation,
  renameConversation: mocks.renameConversation,
  expertTeamTemplateRefresh: mocks.expertTeamTemplateRefresh,
}))

vi.mock('@/stores/uiStore', () => ({
  useUiStore: (selector: (state: unknown) => unknown) =>
    selector({ setRoute: vi.fn(), setSidebarTab: vi.fn() }),
}))

vi.mock('@/stores/notificationStore', () => ({
  useNotificationStore: (selector: (state: unknown) => unknown) =>
    selector({ push: mocks.pushNotification }),
}))

vi.mock('@/stores/chatStore', () => ({
  useChatStore: {
    getState: () => ({ conversations: [], setConversations: vi.fn() }),
  },
}))

vi.mock('./expertTeamRegistry', () => ({
  setExpertTeam: vi.fn(async () => undefined),
}))

import { ExpertTeamsPage } from './ExpertTeamsPage'

describe('ExpertTeamsPage', () => {
  beforeEach(() => {
    void i18n.changeLanguage('zh-CN')
    mocks.expertTeamTemplateRefresh.mockClear()
    mocks.pushNotification.mockClear()
  })

  it('lets users manually sync expert teams from the server', async () => {
    mocks.expertTeamTemplateRefresh.mockResolvedValueOnce(2)
    render(<ExpertTeamsPage />)

    fireEvent.click(screen.getByRole('button', { name: '同步服务端' }))

    await waitFor(() => {
      expect(mocks.expertTeamTemplateRefresh).toHaveBeenCalledTimes(1)
    })
    expect(mocks.pushNotification).toHaveBeenCalledWith(expect.objectContaining({
      level: 'success',
      title: '同步完成，更新 2 个专家团',
    }))
  })

  it('renders localized expert team names in English', async () => {
    await i18n.changeLanguage('en-US')
    render(<ExpertTeamsPage />)

    expect(screen.getByText('Marketing Planning Team')).toBeInTheDocument()
    expect(screen.queryByText('市场营销策划团')).toBeNull()
  })
})
