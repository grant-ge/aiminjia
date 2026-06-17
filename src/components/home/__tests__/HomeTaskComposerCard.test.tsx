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
