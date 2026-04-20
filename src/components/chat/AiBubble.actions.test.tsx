import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { Message } from '@/types/message'

const sendMessageMock = vi.fn(() => Promise.resolve())

vi.mock('@/lib/tauri', () => ({
  sendMessage: (...args: Parameters<typeof sendMessageMock>) => sendMessageMock(...args),
  openGeneratedFile: vi.fn(),
  revealFileInFolder: vi.fn(),
}))

vi.mock('@/stores/chatStore', () => ({
  useChatStore: vi.fn((selector: (state: {
    activeConversationId: string | null
    messages: Message[]
  }) => unknown) =>
    selector({
      activeConversationId: 'conv-1',
      messages: [
        {
          id: 'u-1',
          conversationId: 'conv-1',
          role: 'user',
          createdAt: '2026-04-19T09:00:00Z',
          content: { text: '第一条问题' },
        },
        {
          id: 'a-1',
          conversationId: 'conv-1',
          role: 'assistant',
          createdAt: '2026-04-19T09:01:00Z',
          content: { text: '第一条回复' },
        },
        {
          id: 'u-2',
          conversationId: 'conv-1',
          role: 'user',
          createdAt: '2026-04-19T09:02:00Z',
          content: { text: '最新问题' },
        },
        {
          id: 'a-2',
          conversationId: 'conv-1',
          role: 'assistant',
          createdAt: '2026-04-19T09:03:00Z',
          content: { text: '最新回复' },
        },
      ],
    })),
}))

vi.mock('@/stores/notificationStore', () => ({
  useNotificationStore: {
    getState: () => ({
      push: vi.fn(),
    }),
  },
}))

vi.mock('@/hooks/useProductName', () => ({
  useProductName: () => 'AIjia',
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (
      key: string,
      fallbackOrOptions?: string | { defaultValue?: string; count?: number },
    ) => {
      if (typeof fallbackOrOptions === 'string') return fallbackOrOptions
      return fallbackOrOptions?.defaultValue ?? key
    },
  }),
  initReactI18next: { type: '3rdParty', init: () => {} },
}))

import { AiBubble } from './AiBubble'

describe('AiBubble regenerate action', () => {
  it('clicking regenerate re-sends nearest previous user message text', () => {
    render(
      <AiBubble
        message={{
          id: 'a-2',
          conversationId: 'conv-1',
          role: 'assistant',
          createdAt: '2026-04-19T09:03:00Z',
          content: { text: '最新回复' },
        }}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: '重新生成' }))

    expect(sendMessageMock).toHaveBeenCalledWith('conv-1', '最新问题')
  })
})
