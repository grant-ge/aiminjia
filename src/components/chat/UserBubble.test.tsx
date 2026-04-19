import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { Message } from '@/types/message'

const sendMessageMock = vi.fn(() => Promise.resolve())

vi.mock('@/lib/tauri', () => ({
  sendMessage: (...args: unknown[]) => sendMessageMock(...args),
}))

vi.mock('@/stores/chatStore', () => ({
  useChatStore: vi.fn(
    (selector: (state: { activeConversationId: string | null }) => unknown) =>
      selector({ activeConversationId: 'conv-1' }),
  ),
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

import { UserBubble } from './UserBubble'

const message: Message = {
  id: 'u-1',
  conversationId: 'conv-1',
  role: 'user',
  createdAt: '2026-04-19T10:00:00Z',
  content: { text: '原始问题' },
}

describe('UserBubble edit and resend', () => {
  it('supports editing current text and re-sending', async () => {
    render(<UserBubble message={message} />)

    fireEvent.click(screen.getByRole('button', { name: '编辑并重发' }))

    const input = screen.getByRole('textbox')
    expect(input).toHaveValue('原始问题')

    fireEvent.change(input, { target: { value: '改后的问题' } })
    fireEvent.click(screen.getByRole('button', { name: '重发' }))

    await waitFor(() => {
      expect(sendMessageMock).toHaveBeenCalledWith('conv-1', '改后的问题')
    })
  })
})
