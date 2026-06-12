import '@testing-library/jest-dom'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import i18n from '@/i18n'
import { useChatStore } from '@/stores/chatStore'
import { useSkillStore } from '@/stores/skillStore'
import { setExpertTeam, clearExpertTeam } from '@/features/expert-teams/expertTeamRegistry'
import { setRemoteExpertTeams } from '@/features/expert-teams/teams'
import { ChatPage } from './ChatPage'

const chatMocks = vi.hoisted(() => ({
  switchConversation: vi.fn(),
  renameConversation: vi.fn(),
  archiveConversation: vi.fn(),
  setConversationPinned: vi.fn(),
  sendUserMessage: vi.fn(),
}))
const tauriMocks = vi.hoisted(() => ({
  exportConversation: vi.fn(),
  revealExportInFolder: vi.fn(),
  getConversationSource: vi.fn(),
  employeeList: vi.fn(),
  openGeneratedFile: vi.fn(),
  saveGeneratedFileAs: vi.fn(),
  saveLocalFileAs: vi.fn(),
  clearConversationSource: vi.fn(),
  setConversationExpertTeam: vi.fn(),
  getTeamOverview: vi.fn(),
  onMessageUpdated: vi.fn(),
  onToolCompleted: vi.fn(),
  workplaceDirectoryCatalog: vi.fn(),
  expertTeamTemplateCatalog: vi.fn(),
  expertTeamTemplateRefresh: vi.fn(),
}))

vi.mock('@/hooks/useChat', () => ({
  useChat: () => chatMocks,
}))

vi.mock('@/hooks/useTeamOverview', () => ({
  useTeamOverview: () => ({ overview: null, loaded: true, refetch: vi.fn() }),
}))

vi.mock('@/lib/tauri', () => ({
  exportConversation: tauriMocks.exportConversation,
  revealExportInFolder: tauriMocks.revealExportInFolder,
  getConversationSource: tauriMocks.getConversationSource,
  employeeList: tauriMocks.employeeList,
  openGeneratedFile: tauriMocks.openGeneratedFile,
  saveGeneratedFileAs: tauriMocks.saveGeneratedFileAs,
  saveLocalFileAs: tauriMocks.saveLocalFileAs,
  clearConversationSource: tauriMocks.clearConversationSource,
  setConversationExpertTeam: tauriMocks.setConversationExpertTeam,
  getTeamOverview: tauriMocks.getTeamOverview,
  onMessageUpdated: tauriMocks.onMessageUpdated,
  onToolCompleted: tauriMocks.onToolCompleted,
  workplaceDirectoryCatalog: tauriMocks.workplaceDirectoryCatalog,
  expertTeamTemplateCatalog: tauriMocks.expertTeamTemplateCatalog,
  expertTeamTemplateRefresh: tauriMocks.expertTeamTemplateRefresh,
}))

vi.mock('@/components/shell/ChatTopBar', () => ({
  ChatTopBar: ({
    title,
    sourceLabel,
    employee,
    onShare,
    shareLabel,
    moreMenuItems,
  }: {
    title: string
    sourceLabel?: string
    employee?: { name: string; role: string; defaultSkillLabel?: string | null }
    onShare?: () => void
    shareLabel?: string
    moreMenuItems?: Array<{
      id: string
      label: string
      icon?: React.ReactNode
      onSelect?: () => void
    }>
  }) => (
    <header data-testid="chat-header">
      {employee ? (
        <span data-testid="chat-employee-label">
          {employee.role} · {employee.name}
        </span>
      ) : title}
      {employee?.defaultSkillLabel ? (
        <span data-testid="chat-default-skill">{employee.defaultSkillLabel}</span>
      ) : null}
      {sourceLabel ? <span data-testid="chat-source-label">{sourceLabel}</span> : null}
      {onShare ? <button onClick={onShare}>{shareLabel ?? '分享'}</button> : null}
      {moreMenuItems?.map((item) => (
        <button key={item.id} onClick={() => item.onSelect?.()}>
          {item.icon}
          {item.label}
        </button>
      ))}
    </header>
  ),
}))

vi.mock('@/components/layout/ChatArea', () => ({
  ChatArea: () => <main data-testid="chat-content" />,
}))

vi.mock('@/components/chat-scene/ChatBottomArea', () => ({
  ChatBottomArea: () => <footer data-testid="chat-footer-input" />,
}))

vi.mock('@/components/chat/RightPanel', () => ({
  RightPanel: () => <div data-testid="right-panel" />,
}))

vi.mock('@/components/team/TeamChatDrawer', () => ({
  TeamChatDrawer: () => <aside data-testid="team-chat-drawer" />,
}))

describe('ChatPage layout', () => {
  beforeEach(async () => {
    chatMocks.switchConversation.mockReset()
    chatMocks.renameConversation.mockReset()
    chatMocks.archiveConversation.mockReset()
    chatMocks.setConversationPinned.mockReset()
    chatMocks.sendUserMessage.mockReset()
    tauriMocks.exportConversation.mockReset()
    tauriMocks.revealExportInFolder.mockReset()
    tauriMocks.getConversationSource.mockReset()
    tauriMocks.employeeList.mockReset()
    tauriMocks.openGeneratedFile.mockReset()
    tauriMocks.clearConversationSource.mockReset()
    tauriMocks.setConversationExpertTeam.mockReset()
    tauriMocks.getTeamOverview.mockReset()
    tauriMocks.onMessageUpdated.mockReset()
    tauriMocks.onToolCompleted.mockReset()
    tauriMocks.workplaceDirectoryCatalog.mockReset()
    tauriMocks.expertTeamTemplateCatalog.mockReset()
    tauriMocks.expertTeamTemplateRefresh.mockReset()
    tauriMocks.clearConversationSource.mockResolvedValue(undefined)
    tauriMocks.setConversationExpertTeam.mockResolvedValue(undefined)
    tauriMocks.getTeamOverview.mockResolvedValue(null)
    tauriMocks.onMessageUpdated.mockResolvedValue(() => undefined)
    tauriMocks.onToolCompleted.mockResolvedValue(() => undefined)
    tauriMocks.workplaceDirectoryCatalog.mockResolvedValue({ schemaVersion: 1, categories: [], items: [] })
    tauriMocks.expertTeamTemplateCatalog.mockResolvedValue([])
    tauriMocks.expertTeamTemplateRefresh.mockResolvedValue(0)
    tauriMocks.getConversationSource.mockReturnValue(new Promise(() => {}))
    tauriMocks.employeeList.mockResolvedValue([])
    await i18n.changeLanguage('zh-CN')
    setRemoteExpertTeams([])
    await clearExpertTeam('conv-layout')
    await clearExpertTeam('conv-team')
    await clearExpertTeam('conv-retro')
    useChatStore.setState({ activeConversationId: null, conversations: [], messages: [] })
    useSkillStore.setState({ skills: [], isLoading: false })
  })


  it('loads messages on reload when route conversation is already active but message cache is empty', async () => {
    useChatStore.setState({
      activeConversationId: 'conv-reload',
      conversations: [{ id: 'conv-reload', title: '刷新恢复', createdAt: '', updatedAt: '', isArchived: false }],
      messages: [],
    })

    render(<ChatPage conversationId="conv-reload" />)

    await waitFor(() => {
      expect(chatMocks.switchConversation).toHaveBeenCalledWith('conv-reload')
    })
  })

  it('sends a generated message for creating a skill from the current conversation', async () => {
    useChatStore.setState({
      activeConversationId: 'conv-create',
      conversations: [{ id: 'conv-create', title: '需求讨论', createdAt: '', updatedAt: '', isArchived: false }],
      messages: [],
    })

    render(<ChatPage conversationId="conv-create" />)

    fireEvent.click(screen.getByRole('button', { name: '总结对话并创建技能' }))

    await waitFor(() => {
      expect(chatMocks.sendUserMessage).toHaveBeenCalledTimes(1)
    })
    expect(chatMocks.sendUserMessage.mock.calls[0][0]).toContain('创建为一个技能')
    expect(chatMocks.sendUserMessage.mock.calls[0][0]).toContain('"type": "skill_created"')
  })

  it('uses the skill center icon for creating a skill from conversation', async () => {
    useChatStore.setState({
      activeConversationId: 'conv-create-icon',
      conversations: [{ id: 'conv-create-icon', title: '需求讨论', createdAt: '', updatedAt: '', isArchived: false }],
      messages: [],
    })

    const { container } = render(<ChatPage conversationId="conv-create-icon" />)

    const button = screen.getByRole('button', { name: '总结对话并创建技能' })
    expect(button.querySelector('.lucide-blocks')).toBeTruthy()
    expect(container.querySelector('.lucide-sparkles')).toBeFalsy()
  })

  it('sends a generated message for creating a scheduled task from the current conversation', async () => {
    useChatStore.setState({
      activeConversationId: 'conv-schedule',
      conversations: [{ id: 'conv-schedule', title: '日报讨论', createdAt: '', updatedAt: '', isArchived: false }],
      messages: [],
    })

    render(<ChatPage conversationId="conv-schedule" />)

    fireEvent.click(screen.getByRole('button', { name: '总结对话并创建定时任务' }))

    await waitFor(() => {
      expect(chatMocks.sendUserMessage).toHaveBeenCalledTimes(1)
    })
    expect(chatMocks.sendUserMessage.mock.calls[0][0]).toContain('创建一个定时任务')
    expect(chatMocks.sendUserMessage.mock.calls[0][0]).toContain('"type": "schedule_created"')
  })

  it('wires pin archive copy and export actions in the header more menu', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.assign(navigator, { clipboard: { writeText } })
    useChatStore.setState({
      activeConversationId: 'conv-actions',
      conversations: [{ id: 'conv-actions', title: '操作会话', createdAt: '', updatedAt: '', isArchived: false, isPinned: false }],
      messages: [],
    })

    render(<ChatPage conversationId="conv-actions" />)

    fireEvent.click(screen.getByRole('button', { name: '置顶对话' }))
    expect(chatMocks.setConversationPinned).toHaveBeenCalledWith('conv-actions', true)

    expect(screen.getAllByRole('button', { name: '导出对话' }).length).toBeGreaterThan(0)

    fireEvent.click(screen.getByRole('button', { name: '复制对话 ID' }))
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('conv-actions'))

    fireEvent.click(screen.getByRole('button', { name: '归档聊天' }))
    expect(chatMocks.archiveConversation).toHaveBeenCalledWith('conv-actions')
  })

  it('renders employee identity from conversation source before the index title is available', async () => {
    tauriMocks.getConversationSource.mockResolvedValue({ kind: 'employee', employeeId: 'emp-salary' })
    tauriMocks.employeeList.mockResolvedValue([{
      id: 'emp-salary',
      name: '方予衡',
      role: '薪酬专家',
      description: '生成薪酬公平性分析报告。',
      avatar: '',
      templateId: null,
      toolWhitelist: [],
      cron: null,
      timezone: 'Asia/Shanghai',
      lifecycle: 'active',
      cronEnabled: false,
      resourceConfig: {},
      systemPromptExtra: null,
      defaultSkillId: 'salary-fairness-v2',
      templateRef: null,
      createdAt: '',
      updatedAt: '',
      lastRunAt: null,
      nextRunAt: null,
    }])
    useSkillStore.setState({
      skills: [{
        id: 'salary-fairness-v2',
        displayName: '薪酬公平性分析 v2',
        displayNameEn: 'Salary Fairness Analysis v2',
        description: '',
        source: 'builtin',
        hasWorkflow: true,
        shortDescription: '',
        shortDescriptionEn: '',
        triggerText: '/salary-fairness-v2',
        category: 'general',
        icon: '',
        updatedAt: null,
      }],
      isLoading: false,
    })
    useChatStore.setState({
      activeConversationId: 'conv-employee',
      conversations: [{ id: 'conv-employee', title: '', createdAt: '', updatedAt: '', isArchived: false }],
      messages: [],
    })

    render(<ChatPage conversationId="conv-employee" />)

    expect(screen.queryByTestId('chat-header')).not.toBeInTheDocument()

    await waitFor(() => {
      expect(screen.getByTestId('chat-employee-label')).toHaveTextContent('薪酬专家 · 方予衡')
    })
    expect(screen.getByTestId('chat-default-skill')).toHaveTextContent('薪酬公平性分析 v2')
  })

  it('renders expert team welcome from conversation source even when the store kind is missing', async () => {
    tauriMocks.getConversationSource.mockResolvedValue({
      kind: 'expertTeam',
      expertTeamId: 'marketing',
    })
    useChatStore.setState({
      activeConversationId: 'conv-source-team',
      conversations: [{
        id: 'conv-source-team',
        title: '专家团: 市场营销策划团',
        createdAt: '',
        updatedAt: '',
        isArchived: false,
      }],
      messages: [],
    })

    render(<ChatPage conversationId="conv-source-team" />)

    await waitFor(() => {
      expect(screen.getByTestId('expert-team-welcome-shell')).toBeInTheDocument()
    })
    expect(screen.queryByTestId('chat-content')).not.toBeInTheDocument()
  })

  it('loads a server-authored expert team welcome when the team is not a local built-in', async () => {
    tauriMocks.getConversationSource.mockResolvedValue({
      kind: 'expertTeam',
      expertTeamId: 'talent-acquisition',
    })
    tauriMocks.workplaceDirectoryCatalog.mockResolvedValue({
      schemaVersion: 1,
      categories: [{
        categoryId: 'hr-admin',
        display: { name: '组织人事', description: '组织、招聘、绩效和薪酬议题' },
        icon: '🏗️',
        color: '#2563eb',
        sortOrder: 10,
        resourceCount: 1,
      }],
      items: [{
        resourceType: 'expert_team_template',
        resourceId: 'talent-acquisition',
        version: '1.3',
        workplaceCategoryId: 'hr-admin',
        sortOrder: 10,
        display: {
          name: '招聘评审团',
          tagline: '岗位画像 / 候选人评审 / 面试设计',
          description: '适合招聘项目推进、候选人复核、面试题设计和录用风险讨论。',
        },
        icon: '🎯',
      }],
    })
    tauriMocks.expertTeamTemplateCatalog.mockResolvedValue([{
      teamId: 'talent-acquisition',
      version: '1.3',
      facilitationStyle: 'rounds',
      displayI18n: {
        'zh-CN': {
          name: '招聘评审团',
          tagline: '岗位画像 / 候选人评审 / 面试设计',
          description: '适合招聘项目推进、候选人复核、面试题设计和录用风险讨论。',
          examples: ['设计销售总监岗位面试方案'],
          composerPlaceholder: '告诉他们你要评审的岗位或候选人...',
        },
      },
      experts: [{
        stableName: 'recruiting-lead',
        name: '宋知澜',
        persona: '关注招聘漏斗、候选人体验和交付节奏',
        emoji: '🎯',
      }],
      directorPromptI18n: {
        'zh-CN': { template: '主持「{{teamName}}」讨论\n{{roster}}\n{{topic}}' },
      },
    }])
    useChatStore.setState({
      activeConversationId: 'conv-remote-team',
      conversations: [{
        id: 'conv-remote-team',
        title: '专家团: 招聘评审团',
        createdAt: '',
        updatedAt: '',
        isArchived: false,
        kind: 'expertTeam',
        sourceLabel: '招聘评审团',
      }],
      messages: [],
    })

    render(<ChatPage conversationId="conv-remote-team" />)

    expect(screen.getByTestId('chat-header')).toHaveTextContent('专家团: 招聘评审团')
    await waitFor(() => {
      expect(screen.getByTestId('expert-team-welcome-shell')).toHaveTextContent('招聘评审团')
    })
    expect(screen.queryByTestId('chat-content')).not.toBeInTheDocument()
  })



  it('does not render a redundant expert team banner above the chat content', async () => {
    useChatStore.setState({
      activeConversationId: 'conv-team',
      conversations: [{ id: 'conv-team', title: '专家团会话', createdAt: '', updatedAt: '', isArchived: false }],
      messages: [{ id: 'm1', conversationId: 'conv-team', role: 'assistant', content: { text: '已有消息' }, createdAt: '' }],
    })
    await setExpertTeam('conv-team', 'marketing')

    render(<ChatPage conversationId="conv-team" />)

    expect(screen.queryByLabelText('关闭专家团')).not.toBeInTheDocument()
  })

  it('uses the localized expert team name for the conversation source chip', async () => {
    useChatStore.setState({
      activeConversationId: 'conv-retro',
      conversations: [{
        id: 'conv-retro',
        title: '专家团会话',
        createdAt: '',
        updatedAt: '',
        isArchived: false,
        kind: 'expertTeam',
        sourceLabel: 'Retrospective Diagnosis Team',
      }],
      messages: [{ id: 'm1', conversationId: 'conv-retro', role: 'assistant', content: { text: '已有消息' }, createdAt: '' }],
    })
    await setExpertTeam('conv-retro', 'retrospective', 'Retrospective Diagnosis Team')

    render(<ChatPage conversationId="conv-retro" />)

    expect(screen.getByTestId('chat-source-label')).toHaveTextContent('复盘归因团')
    expect(screen.queryByText('Retrospective Diagnosis Team')).not.toBeInTheDocument()
  })

  it('composes the chat column as header, content, and footer using flex layout', () => {
    useChatStore.setState({
      activeConversationId: 'conv-layout',
      conversations: [{ id: 'conv-layout', title: '布局测试', createdAt: '', updatedAt: '', isArchived: false }],
    })

    render(<ChatPage conversationId="conv-layout" />)

    const column = screen.getByTestId('chat-layout-column')
    expect(column).toHaveClass('flex')
    expect(column).toHaveClass('flex-col')
    expect(column).toHaveClass('overflow-hidden')

    expect(screen.getByTestId('chat-header')).toBeInTheDocument()
    expect(screen.getByTestId('chat-content')).toBeInTheDocument()
    expect(screen.getByTestId('chat-footer-input')).toBeInTheDocument()
    expect(screen.queryByTestId('right-panel')).not.toBeInTheDocument()
  })

  it('asks for confirmation before exporting the current conversation', async () => {
    tauriMocks.exportConversation.mockResolvedValue({
      zipPath: '/tmp/aijia-export.zip',
      fileName: 'aijia-export.zip',
      sizeBytes: 2048,
    })
    tauriMocks.revealExportInFolder.mockResolvedValue(undefined)
    useChatStore.setState({
      activeConversationId: 'conv-export',
      conversations: [{ id: 'conv-export', title: '导出测试', createdAt: '', updatedAt: '', isArchived: false }],
      messages: [],
    })

    render(<ChatPage conversationId="conv-export" />)
    fireEvent.click(screen.getAllByRole('button', { name: '导出对话' })[0])

    expect(tauriMocks.exportConversation).not.toHaveBeenCalled()
    expect(screen.getByText('将生成一个本地 zip 文件，包含当前对话和最近 24 小时运行信息。文件只会保存在本机。')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '开始导出' }))

    await waitFor(() => {
      expect(tauriMocks.exportConversation).toHaveBeenCalledWith('conv-export')
    })
    expect(await screen.findByText('aijia-export.zip')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '打开所在文件夹' }))
    await waitFor(() => {
      expect(tauriMocks.revealExportInFolder).toHaveBeenCalledWith('/tmp/aijia-export.zip')
    })
  })

  it('drops stale export results after switching conversations', async () => {
    let resolveFirstExport!: (result: { zipPath: string; fileName: string; sizeBytes: number }) => void
    const firstExport = new Promise<{ zipPath: string; fileName: string; sizeBytes: number }>((resolve) => {
      resolveFirstExport = resolve
    })
    tauriMocks.exportConversation
      .mockReturnValueOnce(firstExport)
      .mockResolvedValueOnce({
        zipPath: '/tmp/conv-b.zip',
        fileName: 'conv-b.zip',
        sizeBytes: 4096,
      })
    useChatStore.setState({
      activeConversationId: 'conv-a',
      conversations: [
        { id: 'conv-a', title: '会话 A', createdAt: '', updatedAt: '', isArchived: false },
        { id: 'conv-b', title: '会话 B', createdAt: '', updatedAt: '', isArchived: false },
      ],
      messages: [],
    })

    const { rerender } = render(<ChatPage conversationId="conv-a" />)
    fireEvent.click(screen.getAllByRole('button', { name: '导出对话' })[0])
    fireEvent.click(screen.getByRole('button', { name: '开始导出' }))
    await waitFor(() => {
      expect(tauriMocks.exportConversation).toHaveBeenCalledWith('conv-a')
    })

    await act(async () => {
      useChatStore.setState({ activeConversationId: 'conv-b' })
      rerender(<ChatPage conversationId="conv-b" />)
    })

    await act(async () => {
      resolveFirstExport({
        zipPath: '/tmp/conv-a.zip',
        fileName: 'conv-a.zip',
        sizeBytes: 2048,
      })
      await firstExport
    })

    expect(screen.queryByText('conv-a.zip')).not.toBeInTheDocument()

    fireEvent.click(screen.getAllByRole('button', { name: '导出对话' })[0])
    fireEvent.click(screen.getByRole('button', { name: '开始导出' }))
    await waitFor(() => {
      expect(tauriMocks.exportConversation).toHaveBeenCalledWith('conv-b')
    })
    expect(await screen.findByText('conv-b.zip')).toBeInTheDocument()
  })
})
