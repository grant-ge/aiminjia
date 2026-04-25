import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { Message } from '@/types/message'


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
  it('renders selected skill as slash command breadcrumb', () => {
    render(
      <UserBubble
        message={{
          ...message,
          content: {
            text: '这个技能有什么用',
            commandText: '/salary-query 这个技能有什么用',
            skillCommand: { id: 'salary-query', label: '薪酬市场查询助手', command: '/salary-query' },
          },
        }}
      />,
    )

    expect(screen.getByText('/salary-query 这个技能有什么用')).toBeInTheDocument()
    expect(screen.getByText('这个技能有什么用')).toBeInTheDocument()
  })

  it('supports editing current text and re-sending', async () => {
    const onResend = vi.fn()
    render(<UserBubble message={message} onResend={onResend} />)

    fireEvent.click(screen.getByRole('button', { name: '编辑并重发' }))

    const input = screen.getByRole('textbox')
    expect(input).toHaveValue('原始问题')

    fireEvent.change(input, { target: { value: '改后的问题' } })
    fireEvent.click(screen.getByRole('button', { name: '重发' }))

    await waitFor(() => {
      expect(onResend).toHaveBeenCalledWith('改后的问题')
    })
  })
})
