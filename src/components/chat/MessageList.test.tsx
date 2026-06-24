import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { MessageList } from './MessageList'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'
import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { getTeamOverview, isGeneratedFileAvailable, isLocalFileAvailable, openGeneratedFile, revealFileInFolder } from '@/lib/tauri'
import type { GeneratedFile, Message } from '@/types/message'
import { useTeamStore } from '@/stores/teamStore'

vi.mock('@/lib/tauri', async () => {
  const actual = await vi.importActual<typeof import('@/lib/tauri')>('@/lib/tauri')
  return {
    ...actual,
    isGeneratedFileAvailable: vi.fn().mockResolvedValue(true),
    isLocalFileAvailable: vi.fn().mockResolvedValue(true),
    openGeneratedFile: vi.fn().mockResolvedValue(undefined),
    revealFileInFolder: vi.fn().mockResolvedValue(undefined),
    getTeamOverview: vi.fn().mockResolvedValue({ conversationId: '', teams: [] }),
    getTeammateTranscript: vi.fn().mockResolvedValue([]),
    onMessageUpdated: vi.fn().mockResolvedValue(() => {}),
    onToolCompleted: vi.fn().mockResolvedValue(() => {}),
  }
})

const openGeneratedFileMock = vi.mocked(openGeneratedFile)
const revealFileInFolderMock = vi.mocked(revealFileInFolder)
const isGeneratedFileAvailableMock = vi.mocked(isGeneratedFileAvailable)
const isLocalFileAvailableMock = vi.mocked(isLocalFileAvailable)
const getTeamOverviewMock = vi.mocked(getTeamOverview)

function generatedFile(overrides: Partial<GeneratedFile> = {}): GeneratedFile {
  return {
    id: 'gf-1',
    title: 'Summary',
    fileName: 'summary.md',
    filePath: '/tmp/summary.md',
    fileType: 'markdown',
    fileSize: 1024,
    category: 'report',
    version: 1,
    isLatest: true,
    createdAt: '2026-04-28T00:00:00Z',
    description: 'Generated summary',
    ...overrides,
  }
}

function messageWithFile(file: GeneratedFile): Message[] {
  return [
    {
      id: 'u-1',
      conversationId: 'conv-1',
      role: 'user',
      createdAt: '2026-04-28T00:00:00Z',
      content: { text: 'create file' },
    },
    {
      id: 'a-1',
      conversationId: 'conv-1',
      role: 'assistant',
      createdAt: '2026-04-28T00:00:01Z',
      content: { text: 'done', generatedFiles: [file] },
    },
  ]
}

function messagesAcrossDays(): Message[] {
  return [
    {
      id: 'u-day-1',
      conversationId: 'conv-1',
      role: 'user',
      createdAt: '2026-04-28T00:00:00Z',
      content: { text: '第一天' },
    },
    {
      id: 'a-day-1',
      conversationId: 'conv-1',
      role: 'assistant',
      createdAt: '2026-04-28T00:00:01Z',
      content: { text: '收到' },
    },
    {
      id: 'u-day-2',
      conversationId: 'conv-1',
      role: 'user',
      createdAt: '2026-04-29T00:00:00Z',
      content: { text: '第二天' },
    },
    {
      id: 'a-day-2',
      conversationId: 'conv-1',
      role: 'assistant',
      createdAt: '2026-04-29T00:00:01Z',
      content: { text: '继续' },
    },
  ]
}

function messagesWithToolReceipt(): Message[] {
  return [
    {
      id: 'u-receipt',
      conversationId: 'conv-1',
      role: 'user',
      createdAt: '2026-04-28T00:00:00Z',
      content: { text: '帮我制定方案' },
    },
    {
      id: 'a-receipt',
      conversationId: 'conv-1',
      role: 'assistant',
      createdAt: '2026-04-28T00:00:01Z',
      content: { text: '' },
      toolCalls: [
        { id: 'ask-receipt', name: 'AskUserQuestion', arguments: { questions: [{ question: '预算范围' }] } },
      ],
    },
    {
      id: 't-receipt',
      conversationId: 'conv-1',
      role: 'tool',
      createdAt: '2026-04-28T00:00:02Z',
      content: { text: '' },
      toolResult: {
        toolCallId: 'ask-receipt',
        name: 'AskUserQuestion',
        content: 'User has answered your questions: "预算范围"="3000-6000". You can now continue with the user\'s answers in mind.',
        isError: false,
      },
    },
  ]
}

function messagesWithLongQuestionToolReceipt(): Message[] {
  const longQuestion = '你的科幻小说想要围绕哪个核心科学概念展开？三体用了「三体问题 + 黑暗森林法则」，你的故事想以什么样的科学点子或理论作为基石？'
  return [
    {
      id: 'u-receipt-long',
      conversationId: 'conv-1',
      role: 'user',
      createdAt: '2026-04-28T00:00:00Z',
      content: { text: '帮我写科幻小说' },
    },
    {
      id: 'a-receipt-long',
      conversationId: 'conv-1',
      role: 'assistant',
      createdAt: '2026-04-28T00:00:01Z',
      content: { text: '' },
      toolCalls: [
        {
          id: 'ask-receipt-long',
          name: 'AskUserQuestion',
          arguments: {
            questions: [
              { question: longQuestion },
              { question: '你的目标读者群体是谁？这会影响故事的科学深度和语言风格。' },
            ],
          },
        },
      ],
    },
    {
      id: 't-receipt-long',
      conversationId: 'conv-1',
      role: 'tool',
      createdAt: '2026-04-28T00:00:02Z',
      content: { text: '' },
      toolResult: {
        toolCallId: 'ask-receipt-long',
        name: 'AskUserQuestion',
        content: `User has answered your questions: "${longQuestion}"="计算科学，AI 的边界", "你的目标读者群体是谁？这会影响故事的科学深度和语言风格。"="硬核科幻迷". You can now continue with the user's answers in mind.`,
        isError: false,
      },
    },
  ]
}

function messagesWithDeniedPermissionReceipt(): Message[] {
  return [
    {
      id: 'u-permission',
      conversationId: 'conv-1',
      role: 'user',
      createdAt: '2026-04-28T00:00:00Z',
      content: { text: 'read a file' },
    },
    {
      id: 'a-permission',
      conversationId: 'conv-1',
      role: 'assistant',
      createdAt: '2026-04-28T00:00:01Z',
      content: { text: '' },
      toolCalls: [
        { id: 'read-permission', name: 'Read', arguments: { file_path: '/private/tmp/secret.txt' } },
      ],
    },
    {
      id: 't-permission',
      conversationId: 'conv-1',
      role: 'tool',
      createdAt: '2026-04-28T00:00:02Z',
      content: { text: '' },
      toolResult: {
        toolCallId: 'read-permission',
        name: 'Read',
        content: '用户拒绝了这个权限申请，并给出调整说明：请改用工作区里的摘要文件。',
        isError: true,
      },
    },
  ]
}

function messagesWithCompletedToolProcess(): Message[] {
  return [
    {
      id: 'u-summary',
      conversationId: 'conv-1',
      role: 'user',
      createdAt: '2026-04-28T00:00:00Z',
      content: { text: '检查 README' },
    },
    {
      id: 'a-progress',
      conversationId: 'conv-1',
      role: 'assistant',
      createdAt: '2026-04-28T00:00:01Z',
      content: { text: '我先读取 README。' },
    },
    {
      id: 'a-tool',
      conversationId: 'conv-1',
      role: 'assistant',
      createdAt: '2026-04-28T00:00:02Z',
      content: { text: '' },
      toolCalls: [
        { id: 'read-summary', name: 'Read', arguments: { file_path: 'README.md' } },
      ],
    },
    {
      id: 't-tool',
      conversationId: 'conv-1',
      role: 'tool',
      createdAt: '2026-04-28T00:00:03Z',
      content: { text: '' },
      toolResult: {
        toolCallId: 'read-summary',
        name: 'Read',
        content: 'README contents',
        isError: false,
        durationMs: 1234,
      },
    },
    {
      id: 'a-final',
      conversationId: 'conv-1',
      role: 'assistant',
      createdAt: '2026-04-28T00:00:04Z',
      content: { text: '已检查 README，并确认项目启动方式。' },
    },
  ]
}

function messagesWithFinalArtifactInMiddle(): Message[] {
  return [
    {
      id: 'u-artifact-middle',
      conversationId: 'conv-1',
      role: 'user',
      createdAt: '2026-04-28T00:00:00Z',
      content: { text: '生成压测报告' },
    },
    {
      id: 'a-artifact-tool',
      conversationId: 'conv-1',
      role: 'assistant',
      createdAt: '2026-04-28T00:00:01Z',
      content: { text: '我先写入压测报告。' },
      toolCalls: [
        { id: 'write-artifact-report', name: 'Write', arguments: { file_path: '/tmp/pilot_tool_stress_3.md' } },
      ],
    },
    {
      id: 't-artifact-tool',
      conversationId: 'conv-1',
      role: 'tool',
      createdAt: '2026-04-28T00:00:02Z',
      content: { text: '' },
      toolResult: {
        toolCallId: 'write-artifact-report',
        name: 'Write',
        content: '{"created":true,"file_path":"/tmp/pilot_tool_stress_3.md"}',
        isError: false,
      },
    },
    {
      id: 'a-artifact-final',
      conversationId: 'conv-1',
      role: 'assistant',
      createdAt: '2026-04-28T00:00:03Z',
      content: {
        text: [
          '第三轮工具压测完成，以下是执行汇总。',
          '',
          '**生成的产物文件：**',
          '![artifact](/tmp/pilot_tool_stress_3.md)',
          '',
          '**数据对比（与上一轮压测的差异）：**',
          '- Markdown 文件数已更新。',
        ].join('\n'),
      },
    },
  ]
}

function messagesWithCompletedTeamSession(): Message[] {
  return [
    {
      id: 'u-team',
      conversationId: 'conv-1',
      role: 'user',
      createdAt: '2026-04-28T00:00:00Z',
      content: { text: '组建专家团做谈判预演' },
    },
    {
      id: 'a-team-create',
      conversationId: 'conv-1',
      role: 'assistant',
      createdAt: '2026-04-28T00:00:01Z',
      content: { text: '先建立专家团。' },
      toolCalls: [
        {
          id: 'team-create',
          name: 'TeamCreate',
          arguments: { team_name: '沟通/谈判预演团' },
        },
      ],
    },
    {
      id: 't-team-create',
      conversationId: 'conv-1',
      role: 'tool',
      createdAt: '2026-04-28T00:00:02Z',
      content: { text: '' },
      toolResult: {
        toolCallId: 'team-create',
        name: 'TeamCreate',
        content: 'Team created',
        isError: false,
      },
    },
    {
      id: 'a-agent',
      conversationId: 'conv-1',
      role: 'assistant',
      createdAt: '2026-04-28T00:00:03Z',
      content: { text: '然后召唤专家入场。' },
      toolCalls: [
        {
          id: 'agent-1',
          name: 'Agent',
          arguments: { name: 'comm-coach', task: '给出谈判建议' },
        },
      ],
    },
    {
      id: 't-agent',
      conversationId: 'conv-1',
      role: 'tool',
      createdAt: '2026-04-28T00:00:04Z',
      content: { text: '' },
      toolResult: {
        toolCallId: 'agent-1',
        name: 'Agent',
        content: '专家已入场',
        isError: false,
      },
    },
    {
      id: 'a-team-final',
      conversationId: 'conv-1',
      role: 'assistant',
      createdAt: '2026-04-28T00:00:05Z',
      content: { text: '最终决策建议如下。' },
    },
  ]
}

function simpleGreetingMessages(): Message[] {
  return [
    {
      id: 'u-hello',
      conversationId: 'conv-1',
      role: 'user',
      createdAt: '2026-04-28T00:00:00Z',
      content: { text: '你好' },
    },
    {
      id: 'a-hello',
      conversationId: 'conv-1',
      role: 'assistant',
      createdAt: '2026-04-28T00:00:01Z',
      content: { text: '你好！' },
    },
  ]
}

function messagesWithBackgroundNotificationAfterCompletedTurn(): Message[] {
  return [
    {
      id: 'u-background',
      conversationId: 'conv-1',
      role: 'user',
      createdAt: '2026-04-28T00:00:00Z',
      content: { text: '执行一个 30 秒等待命令' },
    },
    {
      id: 'a-background-tool',
      conversationId: 'conv-1',
      role: 'assistant',
      createdAt: '2026-04-28T00:00:01Z',
      content: { text: '' },
      toolCalls: [
        { id: 'wait-background', name: 'Bash', arguments: { command: 'sleep 30' } },
      ],
    },
    {
      id: 't-background',
      conversationId: 'conv-1',
      role: 'tool',
      createdAt: '2026-04-28T00:00:02Z',
      content: { text: '' },
      toolResult: {
        toolCallId: 'wait-background',
        name: 'Bash',
        content: '{"status":"backgrounded"}',
        isError: false,
      },
    },
    {
      id: 'a-background-final',
      conversationId: 'conv-1',
      role: 'assistant',
      createdAt: '2026-04-28T00:00:03Z',
      content: { text: '30 秒等待命令已在后台执行。' },
    },
    ...messagesWithCompletedToolProcess().map((message) => ({
      ...message,
      createdAt: message.createdAt.replace('2026-04-28T00:00', '2026-04-28T00:01'),
    })),
    {
      id: 'u-task-notification',
      conversationId: 'conv-1',
      role: 'user',
      createdAt: '2026-04-28T00:02:00Z',
      content: {
        text: [
          '<task-notification>',
          '  <task-id>wait-background</task-id>',
          '  <status>completed</status>',
          '</task-notification>',
        ].join('\n'),
      },
    },
    {
      id: 'a-task-notification',
      conversationId: 'conv-1',
      role: 'assistant',
      createdAt: '2026-04-28T00:02:01Z',
      content: { text: '之前提交的 30 秒等待命令已执行完毕，正常退出。' },
    },
  ]
}

function resetStores(activeConversationId: string | null = 'conv-1') {
  useChatStore.setState({
    conversations: [],
    activeConversationId,
    messages: [],
    busyConversations: new Set(),
    streamStates: {},
    taskStates: {},
    isStreaming: false,
    streamingContent: '',
    toolExecutions: [],
  })
  useGeneratedFilePreviewStore.setState({ target: null })
  useNotificationStore.setState({ notifications: [] })
  useTeamStore.getState().reset()
}

function renderWithFile(file: GeneratedFile, activeConversationId: string | null = 'conv-1') {
  resetStores(activeConversationId)
  useChatStore.setState({ messages: messageWithFile(file) })
  render(<MessageList />)
}

async function openActionsMenu() {
  fireEvent.pointerDown(await screen.findByRole('button', { name: '更多操作：Summary' }))
}

beforeEach(() => {
  vi.clearAllMocks()
  isGeneratedFileAvailableMock.mockResolvedValue(true)
  isLocalFileAvailableMock.mockResolvedValue(true)
  openGeneratedFileMock.mockResolvedValue(undefined)
  revealFileInFolderMock.mockResolvedValue(undefined)
  getTeamOverviewMock.mockResolvedValue({ conversationId: 'conv-1', teams: [] })
  resetStores()
})

describe('MessageList generated file actions', () => {
  it('collapses a completed tool process when a final assistant answer is present', () => {
    resetStores('conv-1')
    useChatStore.setState({ messages: messagesWithCompletedToolProcess() })

    render(<MessageList />)

    expect(screen.getByText('检查 README')).toBeInTheDocument()
    const processSummary = screen.getByRole('button', { name: '已完成 · 1 步' })
    const summary = screen.getByText('已检查 README，并确认项目启动方式。')
    expect(processSummary).toBeInTheDocument()
    expect(summary).toBeInTheDocument()
    expect(processSummary.compareDocumentPosition(summary)).toBe(Node.DOCUMENT_POSITION_FOLLOWING)
    expect(screen.queryByRole('button', { name: '执行过程' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '查看执行过程' })).not.toBeInTheDocument()
    expect(screen.queryByText('我先读取 README。')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '读取了 1 个文件' })).not.toBeInTheDocument()

    fireEvent.click(processSummary)

    const processBody = screen.getByTestId('completed-process-body')
    expect(processBody).not.toHaveClass('border-l')
    expect(processBody.className).not.toContain('pl-')
    expect(processBody.className).not.toContain('ml-')
    expect(screen.getByText('我先读取 README。')).toBeInTheDocument()
    expect(screen.getByText('已检查 README，并确认项目启动方式。')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '读取了 1 个文件' })).toBeInTheDocument()
  })

  it('keeps final artifact cards at their original position in a collapsed completed turn', async () => {
    resetStores('conv-1')
    useChatStore.setState({ messages: messagesWithFinalArtifactInMiddle() })

    render(<MessageList />)

    const processSummary = screen.getByRole('button', { name: '已完成 · 1 步' })
    const artifactIntro = screen.getByText('生成的产物文件：')
    const artifactCard = await screen.findByTestId('generated-file-card')
    const comparison = screen.getByText('数据对比（与上一轮压测的差异）：')

    expect(processSummary).toBeInTheDocument()
    expect(screen.queryByText('我先写入压测报告。')).not.toBeInTheDocument()
    expect(artifactIntro.compareDocumentPosition(artifactCard)).toBe(Node.DOCUMENT_POSITION_FOLLOWING)
    expect(artifactCard.compareDocumentPosition(comparison)).toBe(Node.DOCUMENT_POSITION_FOLLOWING)
  })

  it('keeps the team session card visible outside the collapsed completed process', async () => {
    resetStores('conv-1')
    getTeamOverviewMock.mockResolvedValue({
      conversationId: 'conv-1',
      teams: [
        {
          teamId: 'expert-team-negotiation',
          teamName: '沟通/谈判预演团',
          createdAt: '2026-04-28T00:00:01Z',
          deletedAt: null,
          members: [
            { agentId: 'lead', agentName: 'team-lead', spawnedAt: '2026-04-28T00:00:01Z', isAsync: false, hasTranscript: false },
            { agentId: 'coach', agentName: 'comm-coach', spawnedAt: '2026-04-28T00:00:03Z', isAsync: true, hasTranscript: true },
          ],
          events: [
            { kind: 'send_message', ts: '2026-04-28T00:00:04Z', from: 'comm-coach', to: 'team-lead', text: 'one', isError: false, toolCallId: 'send-1', variant: 'text' },
          ],
        },
      ],
    })
    useChatStore.setState({ messages: messagesWithCompletedTeamSession() })

    render(<MessageList />)

    const processSummary = screen.getByRole('button', { name: '已完成 · 1 步' })
    const teamCardTitle = await screen.findByText('沟通/谈判预演团')
    const finalSummary = screen.getByText('最终决策建议如下。')

    expect(processSummary).toBeInTheDocument()
    expect(screen.queryByText('先建立专家团。')).not.toBeInTheDocument()
    expect(screen.queryByText('然后召唤专家入场。')).not.toBeInTheDocument()
    expect(teamCardTitle.compareDocumentPosition(finalSummary)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    )
  })

  it('keeps a simple completed chat turn expanded when no summary is present', () => {
    resetStores('conv-1')
    useChatStore.setState({ messages: simpleGreetingMessages() })

    render(<MessageList />)

    expect(screen.getByText('你好')).toBeInTheDocument()
    expect(screen.getByText('你好！')).toBeInTheDocument()
  })

  it('renders a background task completion under the previous turn summary without folding it', () => {
    resetStores('conv-1')
    useChatStore.setState({ messages: messagesWithBackgroundNotificationAfterCompletedTurn() })

    render(<MessageList />)

    const turnSummary = screen.getByText('已检查 README，并确认项目启动方式。')
    const backgroundCompletion = screen.getByText('之前提交的 30 秒等待命令已执行完毕，正常退出。')

    expect(turnSummary).toBeInTheDocument()
    expect(backgroundCompletion).toBeInTheDocument()
    expect(screen.queryByText(/task-notification/)).not.toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: /已完成/ })).toHaveLength(2)
    expect(backgroundCompletion.closest('[data-testid="chat-row"]')).toBe(
      turnSummary.closest('[data-testid="chat-row"]'),
    )
    expect(turnSummary.compareDocumentPosition(backgroundCompletion)).toBe(Node.DOCUMENT_POSITION_FOLLOWING)
  })

  it('does not render day divider bars in the message flow', () => {
    resetStores('conv-1')
    useChatStore.setState({ messages: messagesAcrossDays() })

    render(<MessageList />)

    expect(screen.queryByTestId('day-divider')).not.toBeInTheDocument()
    expect(screen.getByText('第一天')).toBeInTheDocument()
    expect(screen.getByText('第二天')).toBeInTheDocument()
  })

  it('renders answered AskUserQuestion inside the tool aggregation row', () => {
    resetStores('conv-1')
    useChatStore.setState({ messages: messagesWithToolReceipt() })

    render(<MessageList />)

    expect(screen.getByRole('button', { name: /询问了用户 1 个问题/ })).toBeInTheDocument()
    expect(screen.getByText('收到：3000-6000')).toBeInTheDocument()
    expect(screen.queryByText('输入')).not.toBeInTheDocument()
    expect(screen.queryByText('输出')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /询问了用户 1 个问题/ }))

    expect(screen.getByText('AskUserQuestion')).toBeInTheDocument()
    expect(screen.queryByText('输入')).not.toBeInTheDocument()
    fireEvent.click(screen.getByText('AskUserQuestion'))

    expect(screen.getByText('输入')).toBeInTheDocument()
    expect(screen.getByText('输出')).toBeInTheDocument()
    expect(screen.getAllByText(/预算范围/).length).toBeGreaterThan(0)
    expect(screen.getAllByText(/3000-6000/).length).toBeGreaterThan(0)
  })

  it('keeps long AskUserQuestion details hidden until the tool row is expanded', () => {
    resetStores('conv-1')
    useChatStore.setState({ messages: messagesWithLongQuestionToolReceipt() })

    render(<MessageList />)

    expect(screen.getByText('询问了用户 2 个问题')).toBeInTheDocument()
    expect(screen.getByText('收到：计算科学，AI 的边界 / 硬核科幻迷')).toBeInTheDocument()
    expect(screen.queryByText('输入')).not.toBeInTheDocument()
    expect(screen.queryByText('输出')).not.toBeInTheDocument()
    expect(screen.queryByText(/你的科幻小说想要围绕哪个核心科学概念展开/)).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: /询问了用户 2 个问题/ }))

    expect(screen.getByText('AskUserQuestion')).toBeInTheDocument()
    expect(screen.getByText('收到：计算科学，AI 的边界 / 硬核科幻迷')).toBeInTheDocument()
    expect(screen.queryByText(/你的科幻小说想要围绕哪个核心科学概念展开/)).not.toBeInTheDocument()
    fireEvent.click(screen.getByText('AskUserQuestion'))

    expect(screen.getAllByText(/计算科学，AI 的边界/).length).toBeGreaterThan(0)
    expect(screen.getAllByText(/你的科幻小说想要围绕哪个核心科学概念展开/).length).toBeGreaterThan(0)
  })

  it('does not render denied permission receipts in the main chat flow', () => {
    resetStores('conv-1')
    useChatStore.setState({ messages: messagesWithDeniedPermissionReceipt() })

    render(<MessageList />)

    expect(screen.queryByText('用户拒绝了这个权限申请')).not.toBeInTheDocument()
    expect(screen.queryByText(/请改用工作区里的摘要文件/)).not.toBeInTheDocument()
    expect(screen.getByText('读取了 1 个文件')).toBeInTheDocument()
  })

  it('uses the generated file owner conversation for preview/open/reveal when active conversation differs', async () => {
    renderWithFile(generatedFile(), 'conv-2')

    fireEvent.click(await screen.findByRole('button', { name: '预览 Summary' }))

    expect(useGeneratedFilePreviewStore.getState().target).toEqual({
      fileId: 'gf-1',
      conversationId: 'conv-1',
      fileName: 'summary.md',
      fileType: 'markdown',
    })

    await openActionsMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: '用默认应用打开' }))
    await waitFor(() => expect(openGeneratedFileMock).toHaveBeenCalledWith('gf-1', 'conv-1'))

    await openActionsMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: '在文件夹中显示' }))
    await waitFor(() => expect(revealFileInFolderMock).toHaveBeenCalledWith('gf-1', 'conv-1'))
  })

  it('clears stale preview target when rendering a different active conversation', () => {
    resetStores('conv-1')
    useGeneratedFilePreviewStore.getState().openPreview({
      fileId: 'old-file',
      conversationId: 'conv-old',
      fileName: 'old.md',
      fileType: 'markdown',
    })

    render(<MessageList />)

    expect(useGeneratedFilePreviewStore.getState().target).toBeNull()
  })

  it('opens previewable file in the generated file preview store from primary action', async () => {
    renderWithFile(generatedFile())

    fireEvent.click(await screen.findByRole('button', { name: '预览 Summary' }))

    expect(useGeneratedFilePreviewStore.getState().target).toEqual({
      fileId: 'gf-1',
      conversationId: 'conv-1',
      fileName: 'summary.md',
      fileType: 'markdown',
    })
    expect(openGeneratedFileMock).not.toHaveBeenCalled()
  })

  it('renders a generated file card even when its indexed file is unavailable', async () => {
    isGeneratedFileAvailableMock.mockResolvedValueOnce(false)

    renderWithFile(generatedFile())

    expect(await screen.findByTestId('generated-file-card')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '预览 Summary' })).toBeInTheDocument()
    expect(isGeneratedFileAvailableMock).not.toHaveBeenCalled()
  })

  it('renders an artifact marker card when its explicit local path is unavailable', async () => {
    isLocalFileAvailableMock.mockResolvedValueOnce(false)
    resetStores('conv-1')
    useChatStore.setState({
      messages: [
        {
          id: 'u-1',
          conversationId: 'conv-1',
          role: 'user',
          createdAt: '2026-04-28T00:00:00Z',
          content: { text: 'create image' },
        },
        {
          id: 'a-1',
          conversationId: 'conv-1',
          role: 'assistant',
          createdAt: '2026-04-28T00:00:01Z',
          content: { text: 'done\n\n![artifact](/tmp/missing.png)' },
        },
      ],
    })

    render(<MessageList />)

    expect(await screen.findByTestId('generated-file-card')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '预览 missing.png' })).toBeInTheDocument()
    expect(isLocalFileAvailableMock).not.toHaveBeenCalled()
  })

  it('opens non-previewable excel file externally from primary action', async () => {
    renderWithFile(
      generatedFile({
        fileName: 'summary.xlsx',
        fileType: 'excel',
        actions: [
          { type: 'preview', enabled: false, label: 'Preview' },
          { type: 'open', enabled: true, label: 'Open' },
          { type: 'reveal', enabled: true, label: 'Reveal' },
        ],
      }),
    )

    fireEvent.click(await screen.findByRole('button', { name: '打开 Summary' }))

    await waitFor(() => expect(openGeneratedFileMock).toHaveBeenCalledWith('gf-1', 'conv-1'))
    expect(useGeneratedFilePreviewStore.getState().target).toBeNull()
  })

  it('reveals a generated file from the dropdown menu', async () => {
    renderWithFile(generatedFile())

    await openActionsMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: '在文件夹中显示' }))

    await waitFor(() => expect(revealFileInFolderMock).toHaveBeenCalledWith('gf-1', 'conv-1'))
  })

  it('pushes an error toast when open external fails', async () => {
    openGeneratedFileMock.mockRejectedValueOnce(new Error('boom'))
    renderWithFile(generatedFile({ fileName: 'summary.xlsx', fileType: 'excel' }))

    fireEvent.click(await screen.findByRole('button', { name: '打开 Summary' }))

    await waitFor(() => {
      expect(useNotificationStore.getState().notifications.at(-1)).toMatchObject({
        level: 'error',
        title: '无法打开文件',
      })
    })
  })

  it('previews using file owner conversation even when active conversation is missing', async () => {
    renderWithFile(generatedFile(), null)

    fireEvent.click(await screen.findByRole('button', { name: '预览 Summary' }))

    expect(useGeneratedFilePreviewStore.getState().target).toEqual({
      fileId: 'gf-1',
      conversationId: 'conv-1',
      fileName: 'summary.md',
      fileType: 'markdown',
    })
    expect(openGeneratedFileMock).not.toHaveBeenCalled()
    expect(revealFileInFolderMock).not.toHaveBeenCalled()
    expect(useNotificationStore.getState().notifications).toEqual([])
  })

  it('opens using file owner conversation even when active conversation is missing', async () => {
    renderWithFile(generatedFile({ fileName: 'summary.xlsx', fileType: 'excel' }), null)

    fireEvent.click(await screen.findByRole('button', { name: '打开 Summary' }))

    await waitFor(() => expect(openGeneratedFileMock).toHaveBeenCalledWith('gf-1', 'conv-1'))
    expect(useNotificationStore.getState().notifications).toEqual([])
  })

  it('reveals using file owner conversation even when active conversation is missing', async () => {
    renderWithFile(generatedFile(), null)

    await openActionsMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: '在文件夹中显示' }))

    await waitFor(() => expect(revealFileInFolderMock).toHaveBeenCalledWith('gf-1', 'conv-1'))
    expect(useNotificationStore.getState().notifications).toEqual([])
  })
})
