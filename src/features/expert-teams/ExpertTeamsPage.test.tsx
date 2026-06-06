import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '@/i18n'
import type { WorkplaceDirectoryResponse } from '@/lib/tauri'

const mocks = vi.hoisted(() => ({
  createConversation: vi.fn(async () => 'conv-team'),
  renameConversation: vi.fn(),
  expertTeamTemplateRefresh: vi.fn(async () => 0),
  workplaceDirectoryCatalog: vi.fn<() => Promise<WorkplaceDirectoryResponse>>(
    async () => ({ schemaVersion: 1, categories: [], items: [] }),
  ),
  pushNotification: vi.fn(),
}))

vi.mock('@/lib/tauri', () => ({
  createConversation: mocks.createConversation,
  renameConversation: mocks.renameConversation,
  expertTeamTemplateRefresh: mocks.expertTeamTemplateRefresh,
  workplaceDirectoryCatalog: mocks.workplaceDirectoryCatalog,
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
    mocks.createConversation.mockReset()
    mocks.createConversation.mockResolvedValue('conv-team')
    mocks.renameConversation.mockClear()
    mocks.expertTeamTemplateRefresh.mockClear()
    mocks.expertTeamTemplateRefresh.mockResolvedValue(0)
    mocks.workplaceDirectoryCatalog.mockClear()
    mocks.workplaceDirectoryCatalog.mockResolvedValue({ schemaVersion: 1, categories: [], items: [] })
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

    expect(await screen.findByText('Marketing Planning Team')).toBeInTheDocument()
    await waitFor(() => {
      expect(mocks.workplaceDirectoryCatalog).toHaveBeenCalled()
    })
    expect(screen.queryByText('市场营销策划团')).toBeNull()
  })

  it('omits placeholder labels for uncategorized expert teams', async () => {
    render(<ExpertTeamsPage />)

    expect(await screen.findByText('市场营销策划团')).toBeInTheDocument()
    expect(screen.queryByText('全部专家团')).not.toBeInTheDocument()
  })

  it('renders server-side expert team groups from the workplace directory', async () => {
    mocks.workplaceDirectoryCatalog.mockResolvedValueOnce({
      schemaVersion: 1,
      categories: [
        {
          categoryId: 'strategy-group',
          display: { name: '战略与经营', description: '重大事项讨论' },
          icon: '🧭',
          color: '#2563eb',
          sortOrder: 10,
          resourceCount: 1,
        },
        {
          categoryId: 'growth-group',
          display: { name: '营销增长', description: '增长议题讨论' },
          icon: '📈',
          color: '#16a34a',
          sortOrder: 20,
          resourceCount: 1,
        },
      ],
      items: [
        {
          resourceType: 'expert_team_template',
          resourceId: 'strategy',
          version: '1.0.0',
          workplaceCategoryId: 'strategy-group',
          sortOrder: 10,
          display: { name: '战略推演团', tagline: '重大决策前的多视角压力测试' },
        },
        {
          resourceType: 'expert_team_template',
          resourceId: 'marketing',
          version: '1.0.0',
          workplaceCategoryId: 'growth-group',
          sortOrder: 10,
          display: { name: '营销增长团', tagline: '增长策略共创' },
        },
      ],
    })

    render(<ExpertTeamsPage />)

    expect(await screen.findByRole('button', { name: '全部' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '战略与经营' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '营销增长' })).toBeInTheDocument()
    expect(screen.getByText('战略推演团')).toBeInTheDocument()
    expect(screen.getByText('营销增长团')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '战略与经营' }))

    expect(screen.getByText('重大事项讨论')).toBeInTheDocument()
    expect(screen.getByText('战略推演团')).toBeInTheDocument()
    expect(screen.queryByText('营销增长团')).not.toBeInTheDocument()
  })

  it('opens details before starting an expert team', async () => {
    render(<ExpertTeamsPage />)

    fireEvent.click(await screen.findByRole('button', { name: /查看 市场营销策划团 详情/ }))
    expect(mocks.createConversation).not.toHaveBeenCalled()
    expect(screen.getByText('适合讨论的议题')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '召唤专家团' }))

    await waitFor(() => {
      expect(mocks.createConversation).toHaveBeenCalledTimes(1)
    })
  })
})
