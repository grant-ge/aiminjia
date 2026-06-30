import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { Message } from '@/types/message'

vi.mock('@/components/rich-content', () => ({
  SubAgentResultCard: () => null,
}))

vi.mock('@/components/data-table', () => ({
  TableView: () => null,
}))

vi.mock('@/components/data-table/mapDataTable', () => ({
  mapDataTableColumns: vi.fn(),
  mapDataTableRows: vi.fn(),
  toTableMeta: vi.fn(),
}))

vi.mock('@/components/chat-scene/AssistantMarkdown', () => ({
  AssistantMarkdown: () => null,
}))

import { AiBubble } from './AiBubble'

const errorMessage: Message = {
  id: 'msg-error',
  conversationId: 'conv-1',
  role: 'assistant',
  createdAt: '2026-06-30T00:00:00Z',
  content: {},
  error: {
    kind: 'execution_error',
    message: '执行失败',
  },
}

describe('AiBubble error callout', () => {
  it('uses explicit rgba error tokens instead of Tailwind color-mix opacity utilities', () => {
    render(<AiBubble message={errorMessage} />)

    const alert = screen.getByRole('alert')
    expect(alert).toHaveClass('bg-[var(--color-semantic-red-bg-light)]')
    expect(alert).toHaveClass('border-[var(--color-semantic-red-border)]')
    expect(alert.className).not.toContain('bg-destructive/')
    expect(alert.className).not.toContain('border-destructive/')
  })
})
