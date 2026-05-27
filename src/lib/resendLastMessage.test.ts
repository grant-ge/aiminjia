import { beforeEach, describe, expect, it, vi } from 'vitest'

const invokeMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

// i18n mock so the toast messages don't require the full provider tree.
vi.mock('@/i18n', () => ({
  default: { t: (_key: string, fallback?: string) => fallback ?? _key },
}))

import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { resendLastUserMessage } from './resendLastMessage'
import type { Message } from '@/types/message'

function userMsg(overrides: Partial<Message> = {}): Message {
  return {
    id: 'msg-user-1',
    conversationId: 'conv-1',
    role: 'user',
    createdAt: '2026-05-26T00:00:00Z',
    content: { text: 'hello' },
    ...overrides,
  } as Message
}

describe('resendLastUserMessage', () => {
  beforeEach(() => {
    invokeMock.mockReset()
    // Default: every invoke resolves. Individual tests can override via
    // mockImplementation when they need a specific cmd to throw.
    invokeMock.mockResolvedValue(undefined)
    useChatStore.setState({
      messages: [],
      streamStates: {},
      busyConversations: new Set(),
    } as Partial<ReturnType<typeof useChatStore.getState>> as never)
    useNotificationStore.setState({ notifications: [] })
  })

  it('replays send_message with the original text and clientMessageId', async () => {
    useChatStore.getState().setMessages([userMsg({ content: { text: 'please retry me' } })])

    await resendLastUserMessage('conv-1')

    // chatStore.setMessages / addBusyConversation / setConversationStreaming
    // call recordDiagnostic → invoke('record_frontend_diagnostic'). Filter
    // those out so we test the contract of resendLastUserMessage alone.
    const sendMessageCalls = invokeMock.mock.calls.filter((c) => c[0] === 'send_message')
    expect(sendMessageCalls).toHaveLength(1)
    const [cmd, args] = sendMessageCalls[0]
    expect(cmd).toBe('send_message')
    expect(args).toMatchObject({
      conversationId: 'conv-1',
      content: 'please retry me',
      clientMessageId: 'msg-user-1',
      attachments: [],
      skillCommand: null,
    })
  })

  it('forwards file attachments that still have a filePath', async () => {
    useChatStore.getState().setMessages([
      userMsg({
        content: {
          text: 'with file',
          files: [
            {
              id: 'f1',
              fileName: 'report.xlsx',
              filePath: '/tmp/report.xlsx',
              fileSize: 1024,
              fileType: 'excel',
              status: 'uploaded',
              kind: 'file',
            },
            {
              // No filePath → must be dropped (backend needs a path).
              id: 'f2',
              fileName: 'detached.pdf',
              fileSize: 512,
              fileType: 'pdf',
              status: 'uploaded',
            },
          ],
        },
      }),
    ])

    await resendLastUserMessage('conv-1')

    const sendMessageCall = invokeMock.mock.calls.find((c) => c[0] === 'send_message')!
    const [, args] = sendMessageCall as [string, { attachments: { id: string }[] }]
    expect(args.attachments).toHaveLength(1)
    expect(args.attachments[0].id).toBe('f1')
  })

  it('marks the conversation as streaming + busy before sending', async () => {
    useChatStore.getState().setMessages([userMsg()])

    await resendLastUserMessage('conv-1')

    const store = useChatStore.getState()
    expect(store.streamStates['conv-1']?.isStreaming).toBe(true)
    expect(store.busyConversations.has('conv-1')).toBe(true)
  })

  it('shows a toast and skips the IPC when no user message is present', async () => {
    await resendLastUserMessage('conv-empty')

    expect(invokeMock).not.toHaveBeenCalled()
    const notifs = useNotificationStore.getState().notifications
    expect(notifs).toHaveLength(1)
    expect(notifs[0].level).toBe('warning')
  })

  it('rolls back streaming + busy flags when the IPC throws', async () => {
    // mockImplementationOnce binds to whichever invoke call lands first.
    // Several `record_frontend_diagnostic` calls beat send_message to it, so
    // bind only on cmd === 'send_message' instead.
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'send_message') return Promise.reject(new Error('network down'))
      return Promise.resolve()
    })
    useChatStore.getState().setMessages([userMsg()])

    await resendLastUserMessage('conv-1')

    const store = useChatStore.getState()
    expect(store.streamStates['conv-1']?.isStreaming).toBe(false)
    expect(store.busyConversations.has('conv-1')).toBe(false)
    const notifs = useNotificationStore.getState().notifications
    expect(notifs.some((n) => n.level === 'error')).toBe(true)
  })

  it('picks the most recent user message when many exist', async () => {
    useChatStore.getState().setMessages([
      userMsg({ id: 'u1', content: { text: 'first' } }),
      { ...userMsg({ id: 'a1' }), role: 'assistant', content: { text: 'mid' } } as Message,
      userMsg({ id: 'u2', content: { text: 'latest' } }),
    ])

    await resendLastUserMessage('conv-1')

    const sendCall = invokeMock.mock.calls.find((c) => c[0] === 'send_message')!
    const sendArgs = sendCall[1] as { content: string; clientMessageId: string }
    expect(sendArgs.content).toBe('latest')
    expect(sendArgs.clientMessageId).toBe('u2')
  })
})
