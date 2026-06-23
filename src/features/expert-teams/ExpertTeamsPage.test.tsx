import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '@/i18n'
import type { ExpertTeamTemplateSnapshot, WorkplaceDirectoryResponse } from '@/lib/tauri'

const mocks = vi.hoisted(() => ({
  createConversation: vi.fn(async () => 'conv-team'),
  renameConversation: vi.fn(),
  workplaceDirectoryCatalog: vi.fn<(
    lang?: string,
    options?: { forceRefresh?: boolean },
  ) => Promise<WorkplaceDirectoryResponse>>(
    async () => ({ schemaVersion: 1, categories: [], items: [] }),
  ),
  expertTeamTemplateCatalog: vi.fn<() => Promise<ExpertTeamTemplateSnapshot[]>>(async () => []),
  expertTeamTemplateRefresh: vi.fn(async () => 0),
  pushNotification: vi.fn(),
}))

vi.mock('@/lib/tauri', () => ({
  createConversation: mocks.createConversation,
  renameConversation: mocks.renameConversation,
  workplaceDirectoryCatalog: mocks.workplaceDirectoryCatalog,
  expertTeamTemplateCatalog: mocks.expertTeamTemplateCatalog,
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

function makeDirectory(): WorkplaceDirectoryResponse {
  return {
    schemaVersion: 1,
    categories: [
      {
        categoryId: 'hr-admin',
        display: { name: '组织人事', description: '组织、招聘、绩效和薪酬议题' },
        icon: '🏗️',
        color: '#2563eb',
        sortOrder: 10,
        resourceCount: 1,
      },
    ],
    items: [
      {
        resourceType: 'expert_team_template',
        resourceId: 'talent-acquisition',
        version: '1.1',
        workplaceCategoryId: 'hr-admin',
        sortOrder: 10,
        display: {
          name: '招聘评审团',
          tagline: '岗位画像 / 候选人评审 / 面试设计',
          description: '围绕岗位画像、候选人评审和面试设计，组织多位专家一起拆解招聘决策。',
        },
        icon: '🎯',
      },
    ],
  }
}

function makeSnapshot(overrides: Partial<ExpertTeamTemplateSnapshot> = {}): ExpertTeamTemplateSnapshot {
  return {
    teamId: 'talent-acquisition',
    version: '1.1',
    facilitationStyle: 'rounds',
    displayI18n: {
      'zh-CN': {
        name: '招聘评审团',
        tagline: '岗位画像 / 候选人评审 / 面试设计',
        description: '结合岗位目标、候选人材料和面试反馈，帮助团队形成更完整的招聘评审意见。',
        examples: ['设计销售总监岗位面试方案'],
        composerPlaceholder: '告诉他们你要评审的岗位或候选人...',
      },
      'en-US': {
        name: 'Talent Acquisition Review Team',
        tagline: 'Role profiles, candidate review, and interview design',
        description: 'A cross-functional hiring review team that compares role goals, candidate evidence, and interview signals before making a recommendation.',
        examples: ['Design an interview plan for a sales director role'],
        composerPlaceholder: 'Tell the team which role or candidates you want to review...',
      },
    },
    experts: [
      {
        stableName: 'recruiting-lead',
        name: '招聘负责人',
        persona: '关注招聘漏斗、候选人体验和交付节奏',
        emoji: '🎯',
        avatarName: '招聘负责人',
        displayI18n: {
          'zh-CN': { name: '招聘负责人', persona: '关注招聘漏斗、候选人体验和交付节奏' },
          'en-US': { name: 'Recruiting Lead', persona: 'Focuses on hiring funnel health and delivery cadence' },
        },
      },
    ],
    directorPromptI18n: {
      'zh-CN': { template: '主持「{{teamName}}」讨论\n{{roster}}\n{{topic}}' },
      'en-US': { template: 'Host "{{teamName}}"\n{{roster}}\n{{topic}}' },
    },
    ...overrides,
  }
}

describe('ExpertTeamsPage', () => {
  beforeEach(() => {
    void i18n.changeLanguage('zh-CN')
    mocks.createConversation.mockReset()
    mocks.createConversation.mockResolvedValue('conv-team')
    mocks.renameConversation.mockClear()
    mocks.workplaceDirectoryCatalog.mockReset()
    mocks.workplaceDirectoryCatalog.mockResolvedValue(makeDirectory())
    mocks.expertTeamTemplateCatalog.mockReset()
    mocks.expertTeamTemplateCatalog.mockResolvedValue([makeSnapshot()])
    mocks.expertTeamTemplateRefresh.mockReset()
    mocks.expertTeamTemplateRefresh.mockResolvedValue(0)
    mocks.pushNotification.mockClear()
  })

  it('lets users manually refresh expert teams from the server directory', async () => {
    render(<ExpertTeamsPage />)

    await waitFor(() => {
      expect(mocks.workplaceDirectoryCatalog).toHaveBeenCalledTimes(1)
    })
    expect(mocks.workplaceDirectoryCatalog).not.toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ forceRefresh: true }),
    )
    mocks.workplaceDirectoryCatalog.mockClear()

    fireEvent.click(screen.getByRole('button', { name: '更新内容' }))

    await waitFor(() => {
      expect(mocks.workplaceDirectoryCatalog).toHaveBeenCalledWith(
        expect.any(String),
        { forceRefresh: true },
      )
    })
    expect(mocks.pushNotification).toHaveBeenCalledWith(expect.objectContaining({
      level: 'success',
      title: '内容已更新',
    }))
  })

  it('renders localized expert team names from server snapshots in English', async () => {
    await i18n.changeLanguage('en-US')
    render(<ExpertTeamsPage />)

    expect(await screen.findByText('Talent Acquisition Review Team')).toBeInTheDocument()
    expect(screen.getByText(/A cross-functional hiring review team.*Recruiting Lead reviews.*hiring funnel health and delivery cadence/)).toBeInTheDocument()
    expect(screen.getByText('1 experts / Round-robin discussion')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /View Talent Acquisition Review Team details/ }))
    expect(screen.getByText('Recruiting Lead')).toBeInTheDocument()
    expect(screen.queryByText('招聘评审团')).toBeNull()
  })

  it('shows an empty server-directory state instead of falling back to local built-ins', async () => {
    mocks.workplaceDirectoryCatalog.mockResolvedValueOnce({ schemaVersion: 1, categories: [], items: [] })
    mocks.expertTeamTemplateCatalog.mockResolvedValueOnce([])

    render(<ExpertTeamsPage />)

    expect(await screen.findByText('暂时没有可用专家团。请更新内容后再试。')).toBeInTheDocument()
    expect(screen.queryByText('市场营销策划团')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '全部' })).not.toBeInTheDocument()
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
          categoryId: 'hr-admin',
          display: { name: '组织人事', description: '组织、招聘、绩效和薪酬议题' },
          icon: '🏗️',
          color: '#16a34a',
          sortOrder: 20,
          resourceCount: 1,
        },
      ],
      items: [
        {
          resourceType: 'expert_team_template',
          resourceId: 'strategy-council',
          version: '1.0',
          workplaceCategoryId: 'strategy-group',
          sortOrder: 10,
          display: { name: '战略推演团', tagline: '重大决策前的多视角压力测试' },
          icon: '🧭',
        },
        {
          resourceType: 'expert_team_template',
          resourceId: 'talent-acquisition',
          version: '1.1',
          workplaceCategoryId: 'hr-admin',
          sortOrder: 10,
          display: { name: '招聘评审团', tagline: '岗位画像 / 候选人评审 / 面试设计' },
          icon: '🎯',
        },
      ],
    })
    mocks.expertTeamTemplateCatalog.mockResolvedValueOnce([
      makeSnapshot({
        teamId: 'strategy-council',
        version: '1.0',
        displayI18n: {
          'zh-CN': {
            name: '战略推演团',
            tagline: '重大决策前的多视角压力测试',
            examples: ['是否启动 B 轮融资'],
            composerPlaceholder: '告诉他们你想推演什么决策...',
          },
        },
      }),
      makeSnapshot(),
    ])

    render(<ExpertTeamsPage />)

    expect(await screen.findByRole('button', { name: '全部' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '战略与经营' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '组织人事' })).toBeInTheDocument()
    expect(screen.getByText('战略推演团')).toBeInTheDocument()
    expect(screen.getByText('招聘评审团')).toBeInTheDocument()
    expect(document.querySelector('.grid.min-w-0')).toHaveClass('lg:grid-cols-3', 'xl:grid-cols-4')

    fireEvent.click(screen.getByRole('button', { name: '战略与经营' }))

    expect(screen.getByText('重大事项讨论')).toBeInTheDocument()
    expect(screen.getByText('战略推演团')).toBeInTheDocument()
    expect(screen.queryByText('招聘评审团')).not.toBeInTheDocument()
  })

  it('opens details before starting an expert team', async () => {
    render(<ExpertTeamsPage />)

    fireEvent.click(await screen.findByRole('button', { name: /查看 招聘评审团 详情/ }))
    expect(mocks.createConversation).not.toHaveBeenCalled()
    expect(screen.getAllByText(/结合岗位目标、候选人材料和面试反馈.*招聘负责人.*招聘漏斗/).length).toBeGreaterThanOrEqual(2)
    expect(document.querySelector('[data-aijia-expert-team-detail]')).toHaveClass('max-w-[680px]', 'gap-0', 'rounded-md')
    expect(document.querySelector('[data-aijia-expert-team-detail-chrome]')).toHaveClass('px-5', 'py-5')
    expect(document.querySelector('[data-aijia-expert-team-detail-logo]')).toBeNull()
    expect(document.querySelector('[data-aijia-expert-team-avatar-stack]')).toBeInTheDocument()
    expect(screen.getByText('基础信息')).toBeInTheDocument()
    expect(screen.getByText('团队成员')).toBeInTheDocument()
    expect(screen.getByText('适合讨论的议题')).toBeInTheDocument()
    expect(screen.getByText('招聘负责人')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '召唤专家团' }))

    await waitFor(() => {
      expect(mocks.createConversation).toHaveBeenCalledTimes(1)
    })
  })
})
