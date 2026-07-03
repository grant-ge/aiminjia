import '@testing-library/jest-dom'
import { StrictMode } from 'react'
import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent, act } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { HomeTaskComposerCard } from '../HomeTaskComposerCard'
import { useChatStore } from '@/stores/chatStore'
import { DRAFT_PERMISSION_SESSION_ID, DRAFT_REASONING_SESSION_ID, useUiStore } from '@/stores/uiStore'
import { useHomeStore } from '@/stores/homeStore'

vi.mock('@tiptap/react', async (importOriginal) => {
  const mod = await importOriginal<typeof import('@tiptap/react')>()
  return { ...mod, ReactNodeViewRenderer: () => () => ({}) }
})

const mockSendUserMessage = vi.fn()
const mockCreateConversation = vi.fn()
const mockAuthorizeLocalDirectory = vi.fn()
const mockGetDefaultFolder = vi.fn()
const mockPickLocalDirectory = vi.fn()
const mockPickAttachments = vi.fn()
const mockGetSettings = vi.fn()
const mockUpdateSettings = vi.fn()

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ sendUserMessage: mockSendUserMessage, isStreaming: false, stopCurrentStream: vi.fn() }),
}))

vi.mock('@/hooks/useChatAttachments', () => ({
  useChatAttachments: () => ({
    isPickingAttachments: false,
    pickAttachments: mockPickAttachments,
    saveClipboardImage: vi.fn(),
    resolvePastedPaths: vi.fn(),
  }),
}))

vi.mock('@/lib/tauri', () => ({
  authorizeLocalDirectory: (...args: unknown[]) => mockAuthorizeLocalDirectory(...args),
  createConversation: () => mockCreateConversation(),
  getDefaultFolder: () => mockGetDefaultFolder(),
  getSettings: () => mockGetSettings(),
  pickLocalDirectory: (opts: unknown) => mockPickLocalDirectory(opts),
  readClipboardFilePaths: vi.fn().mockResolvedValue([]),
  saveClipboardImageToWorkspaceStaging: vi.fn(),
  updateSettings: (settings: unknown) => mockUpdateSettings(settings),
}))

beforeEach(() => {
  mockSendUserMessage.mockReset().mockResolvedValue(undefined)
  mockCreateConversation.mockReset().mockResolvedValue('new-conv-1')
  mockAuthorizeLocalDirectory.mockReset().mockResolvedValue(undefined)
  mockGetDefaultFolder.mockReset().mockResolvedValue({ id: 'default', rootPath: '/home', displayName: '默认' })
  mockPickLocalDirectory.mockReset()
  mockPickAttachments.mockReset().mockResolvedValue([])
  mockGetSettings.mockReset().mockResolvedValue({})
  mockUpdateSettings.mockReset().mockResolvedValue(undefined)
  useChatStore.setState({ activeConversationId: null, conversations: [], messages: [] })
  useUiStore.setState({
    route: { kind: 'home' },
    prefillText: undefined,
    permissionModesBySession: {},
    reasoningModesBySession: {},
  })
  useHomeStore.setState({ selectedWorkspace: null, recentWorkspaces: [] })
})

describe('HomeTaskComposerCard', () => {
  it('renders RichComposer', async () => {
    render(<HomeTaskComposerCard />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
  })

  it('clears and fills the composer from quick prompt actions without submitting', async () => {
    const user = userEvent.setup()
    const { rerender } = render(<HomeTaskComposerCard />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement

    await user.click(editor)
    await user.type(editor, 'existing draft')
    rerender(
      <HomeTaskComposerCard
        quickPrompt={{
          mode: 'clear',
          prompt: '',
        }}
      />,
    )

    await waitFor(() => {
      expect(editor).not.toHaveTextContent('existing draft')
    })

    rerender(
      <HomeTaskComposerCard
        quickPrompt={{
          mode: 'fill',
          prompt: '把这周的目标拆成每日任务，标注优先级、依赖关系和验收标准。',
        }}
      />,
    )

    await waitFor(() => {
      expect(editor).toHaveTextContent('把这周的目标拆成每日任务')
    })
    expect(editor).not.toHaveTextContent('existing draft')
    expect(mockCreateConversation).not.toHaveBeenCalled()
    expect(mockSendUserMessage).not.toHaveBeenCalled()
  })

  it('shows a workspace hint for quick prompts that need project code and lets the user dismiss it', async () => {
    const user = userEvent.setup()
    render(
      <HomeTaskComposerCard
        quickPrompt={{
          mode: 'fill',
          prompt: '请阅读当前代码工作区，生成一份新手上手指南。',
          requiresWorkspace: true,
        }}
      />,
    )

    expect(await screen.findByTestId('home-workspace-required-hint'))
      .toHaveTextContent('这个示例需要读取项目代码')

    await user.click(screen.getByRole('button', { name: '关闭项目目录提示' }))

    await waitFor(() => {
      expect(screen.queryByTestId('home-workspace-required-hint')).not.toBeInTheDocument()
    })
  })

  it('hides the workspace hint when the user edits the filled quick prompt', async () => {
    const user = userEvent.setup()
    render(
      <HomeTaskComposerCard
        quickPrompt={{
          mode: 'fill',
          prompt: '请根据报错「xxx」在当前代码工作区中定位可能原因。',
          requiresWorkspace: true,
        }}
      />,
    )

    expect(await screen.findByTestId('home-workspace-required-hint')).toBeInTheDocument()

    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, ' 补充说明')

    await waitFor(() => {
      expect(screen.queryByTestId('home-workspace-required-hint')).not.toBeInTheDocument()
    })
  })

  it('does not show the workspace hint when an explicit project directory is already selected', async () => {
    useHomeStore.setState({
      selectedWorkspace: { id: 'lotus-app', rootPath: '/Users/me/lotus-app', displayName: 'lotus-app' },
      recentWorkspaces: [],
    })

    render(
      <HomeTaskComposerCard
        quickPrompt={{
          mode: 'fill',
          prompt: '请评审当前代码工作区的未提交改动。',
          requiresWorkspace: true,
        }}
      />,
    )

    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    expect(screen.queryByTestId('home-workspace-required-hint')).not.toBeInTheDocument()
  })

  it('does not overwrite edited quick prompt text when selecting a workspace', async () => {
    const user = userEvent.setup()
    useHomeStore.setState({
      selectedWorkspace: null,
      recentWorkspaces: [
        { id: 'lotus-app', rootPath: '/Users/me/lotus-app', displayName: 'lotus-app' },
      ],
    })
    render(
      <HomeTaskComposerCard
        quickPrompt={{
          mode: 'fill',
          prompt: '请阅读当前代码工作区，生成一份新手上手指南。',
          requirements: ['workspace'],
        }}
      />,
    )

    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await waitFor(() => expect(editor).toHaveTextContent('请阅读当前代码工作区'))
    await user.click(editor)
    await user.type(editor, ' 我补充了一句')
    expect(editor).toHaveTextContent('我补充了一句')

    fireEvent.pointerDown(await screen.findByRole('button', { name: /选择工作目录/ }))
    fireEvent.click(await screen.findByRole('menuitem', { name: /lotus-app/ }))

    await waitFor(() => {
      expect(screen.queryByTestId('home-workspace-required-hint')).not.toBeInTheDocument()
    })
    expect(editor).toHaveTextContent('我补充了一句')
  })

  it('shows an attachment hint for quick prompts that need Excel data and lets the user dismiss it', async () => {
    const user = userEvent.setup()
    render(
      <HomeTaskComposerCard
        quickPrompt={{
          mode: 'fill',
          prompt: '我会上传一份 Excel 数据，请帮我完成数据校验。',
          requirements: ['excelAttachment'],
        }}
      />,
    )

    expect(await screen.findByTestId('home-attachment-required-hint'))
      .toHaveTextContent('这个示例需要 Excel 数据')

    await user.click(screen.getByRole('button', { name: '关闭上传提示' }))

    await waitFor(() => {
      expect(screen.queryByTestId('home-attachment-required-hint')).not.toBeInTheDocument()
    })
  })

  it('hides the attachment hint after the user uploads an attachment', async () => {
    mockPickAttachments.mockResolvedValueOnce([
      {
        id: 'sheet-1',
        fileName: 'salary.xlsx',
        path: '/tmp/salary.xlsx',
        kind: 'file',
        fileType: 'xlsx',
        fileSize: 0,
        mimeType: undefined,
        source: 'picker',
      },
    ])
    const { container } = render(
      <HomeTaskComposerCard
        quickPrompt={{
          mode: 'fill',
          prompt: '请先提示我上传 Excel，然后做人群分析。',
          requirements: ['excelAttachment'],
        }}
      />,
    )

    expect(await screen.findByTestId('home-attachment-required-hint')).toBeInTheDocument()

    const attachBtn = container.querySelector('[aria-label="添加附件"]') as HTMLElement
    await act(async () => {
      attachBtn.click()
    })

    await waitFor(() => {
      expect(screen.queryByTestId('home-attachment-required-hint')).not.toBeInTheDocument()
    })
    expect(document.querySelector('.ProseMirror')?.innerHTML ?? '').toContain('salary.xlsx')
  })

  it('inserts a pending skill chip only once under StrictMode', async () => {
    useUiStore.setState({
      pendingSkill: {
        id: 'biz-proposal',
        label: '商业方案撰写',
        trigger: '/biz-proposal',
      },
    })

    render(
      <StrictMode>
        <HomeTaskComposerCard />
      </StrictMode>,
    )

    await waitFor(() => {
      expect(document.querySelectorAll('[data-rich-composer-skill-token]')).toHaveLength(1)
    })
  })

  it('Enter with text → creates conversation, switches route, sends message', async () => {
    const user = userEvent.setup()
    render(<HomeTaskComposerCard />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'analyze sales')
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalled())
    expect(mockCreateConversation).toHaveBeenCalled()
    expect(useChatStore.getState().activeConversationId).toBe('new-conv-1')
    expect(useUiStore.getState().route).toEqual({ kind: 'chat', conversationId: 'new-conv-1' })
    expect(mockSendUserMessage.mock.calls[0][0]).toBe('analyze sales')
  })

  it('binds the selected permission mode to the new conversation and sends with it', async () => {
    const user = userEvent.setup()
    render(<HomeTaskComposerCard />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())

    await user.click(screen.getByRole('button', { name: '权限模式：默认' }))
    expect(await screen.findByText('完全访问权限说明')).toBeInTheDocument()
    await user.click(screen.getByText('完全访问权限'))

    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'go')
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })

    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalledTimes(1))
    expect(mockSendUserMessage.mock.calls[0][3]).toBe('fullAccess')
    expect(useUiStore.getState().permissionModesBySession[DRAFT_PERMISSION_SESSION_ID]).toBe('fullAccess')
    expect(useUiStore.getState().permissionModesBySession['new-conv-1']).toBe('fullAccess')
  })

  it('binds the selected reasoning mode to the new conversation and sends with it', async () => {
    const user = userEvent.setup()
    render(<HomeTaskComposerCard />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())

    await user.click(screen.getByRole('button', { name: '思考模式：自动' }))
    await user.click(await screen.findByText('深度思考'))

    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'deep task')
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })

    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalledTimes(1))
    expect(mockSendUserMessage.mock.calls[0][4]).toBe('deep')
    expect(useUiStore.getState().reasoningModesBySession[DRAFT_REASONING_SESSION_ID]).toBe('deep')
    expect(useUiStore.getState().reasoningModesBySession['new-conv-1']).toBe('deep')
  })

  it('attachment via picker shows token + Enter sends with file refs', async () => {
    mockPickAttachments.mockResolvedValueOnce([
      {
        id: 'p1',
        fileName: 'plan.pdf',
        path: '/p/plan.pdf',
        kind: 'file',
        fileType: 'pdf',
        fileSize: 0,
        mimeType: undefined,
        source: 'picker',
      },
    ])
    const { container } = render(<HomeTaskComposerCard />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const attachBtn = container.querySelector('[aria-label="添加附件"]') as HTMLElement
    await act(async () => {
      attachBtn.click()
    })
    await waitFor(() => {
      const html = document.querySelector('.ProseMirror')?.innerHTML ?? ''
      expect(html).toContain('plan.pdf')
    })
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalled())
    const [text, files] = mockSendUserMessage.mock.calls[0]
    expect(text).toContain('[附件: plan.pdf](<file:///p/plan.pdf>)')
    expect(files).toHaveLength(1)
    expect(files[0].filePath).toBe('/p/plan.pdf')
  })

  it('renders home-only workspace bar below a larger composer without using RichComposer project button', async () => {
    render(<HomeTaskComposerCard />)
    const homeShell = await screen.findByTestId('home-composer-shell')
    const workspaceBar = await screen.findByTestId('home-workspace-bar')
    const workspaceButton = await screen.findByRole('button', { name: /选择工作目录/ })

    expect(homeShell).toHaveClass('home-composer-large')
    expect(workspaceBar).toHaveClass('absolute')
    expect(workspaceBar).toHaveClass('border-x')
    expect(workspaceBar).toHaveClass('border-b')
    expect(workspaceBar).toHaveClass('bg-sidebar')
    expect(workspaceButton).toHaveTextContent('在 默认 中工作')
    expect(workspaceButton).toHaveAttribute('title', '/home')
    expect(screen.queryByRole('button', { name: /^默认$/ })).not.toBeInTheDocument()
  })

  it('opens a workspace dropdown with recent folders and choose-different action', async () => {
    useHomeStore.setState({
      selectedWorkspace: { id: 'txl', rootPath: '/Users/me/txl', displayName: 'txl' },
      recentWorkspaces: [
        { id: '账单核对', rootPath: '/Users/me/Desktop/账单核对', displayName: '账单核对' },
        { id: 'lotus-app', rootPath: '/Users/me/lotus-app', displayName: 'lotus-app' },
      ],
    })
    render(<HomeTaskComposerCard />)

    fireEvent.pointerDown(await screen.findByRole('button', { name: /选择工作目录/ }))

    expect(await screen.findByRole('menuitem', { name: /账单核对/ })).toBeInTheDocument()
    expect(screen.getByRole('menuitem', { name: /账单核对/ })).toHaveAttribute('title', '/Users/me/Desktop/账单核对')
    expect(screen.getByRole('menuitem', { name: /lotus-app/ })).toBeInTheDocument()
    expect(screen.getByRole('menuitem', { name: /选择其他目录/ })).toBeInTheDocument()
  })

  it('selects a recent workspace from the dropdown without opening the system picker', async () => {
    useHomeStore.setState({
      selectedWorkspace: { id: 'txl', rootPath: '/Users/me/txl', displayName: 'txl' },
      recentWorkspaces: [
        { id: '账单核对', rootPath: '/Users/me/Desktop/账单核对', displayName: '账单核对' },
      ],
    })
    render(<HomeTaskComposerCard />)

    fireEvent.pointerDown(await screen.findByRole('button', { name: /选择工作目录/ }))
    fireEvent.click(await screen.findByRole('menuitem', { name: /账单核对/ }))

    expect(mockPickLocalDirectory).not.toHaveBeenCalled()
    expect(await screen.findByText('在 账单核对 中工作')).toBeInTheDocument()
    expect(useHomeStore.getState().selectedWorkspace?.rootPath).toBe('/Users/me/Desktop/账单核对')
  })

  it('choose different folder from dropdown opens picker and stores it as most recent', async () => {
    mockPickLocalDirectory.mockResolvedValueOnce('/Users/me/lotus-app')
    useHomeStore.setState({
      selectedWorkspace: { id: 'txl', rootPath: '/Users/me/txl', displayName: 'txl' },
      recentWorkspaces: [
        { id: '账单核对', rootPath: '/Users/me/Desktop/账单核对', displayName: '账单核对' },
      ],
    })
    render(<HomeTaskComposerCard />)

    fireEvent.pointerDown(await screen.findByRole('button', { name: /选择工作目录/ }))
    fireEvent.click(await screen.findByRole('menuitem', { name: /选择其他目录/ }))

    await waitFor(() => {
      expect(mockPickLocalDirectory).toHaveBeenCalledWith({
        defaultPath: '/Users/me/txl',
        title: '选择工作目录',
      })
    })
    expect(await screen.findByText('在 lotus-app 中工作')).toBeInTheDocument()
    expect(useHomeStore.getState().recentWorkspaces.map((ws) => ws.displayName)).toEqual([
      'lotus-app',
      '账单核对',
    ])
  })

  it('non-default workspace → authorizeLocalDirectory called before send', async () => {
    useHomeStore.setState({
      selectedWorkspace: { id: 'proj', rootPath: '/Users/me/proj', displayName: 'proj' },
    })
    const user = userEvent.setup()
    render(<HomeTaskComposerCard />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'go')
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalled())
    expect(mockAuthorizeLocalDirectory).toHaveBeenCalledWith('/Users/me/proj', 'new-conv-1')
  })

  it('default workspace → authorizeLocalDirectory NOT called', async () => {
    // selectedWorkspace null → getDefaultFolder returns id=default
    const user = userEvent.setup()
    render(<HomeTaskComposerCard />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'go')
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalled())
    expect(mockAuthorizeLocalDirectory).not.toHaveBeenCalled()
  })
})
