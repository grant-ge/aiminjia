import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, it, expect, beforeEach, vi } from 'vitest'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (k: string, opts?: { count?: number }) =>
      opts?.count != null ? `${k}:${opts.count}` : k,
  }),
}))

import { PendingChips } from './PendingChips'
import { usePendingStore } from '@/stores/pendingStore'
import type { PendingItem } from '@/types/pending'

const item = (id: string): PendingItem => ({
  id,
  source: 'app',
  text: `text-${id}`,
  senderNick: null,
  attachments: [],
  receivedAt: '2026-05-11T03:21:00Z',
})

describe('PendingChips', () => {
  beforeEach(() => {
    usePendingStore.setState({ bySession: {} })
  })

  it('renders nothing when no items', () => {
    const { container } = render(<PendingChips sessionId="s1" />)
    expect(container.firstChild).toBeNull()
  })

  it('renders single hint with 1 item', () => {
    usePendingStore.setState({ bySession: { s1: [item('a')] } })
    render(<PendingChips sessionId="s1" />)
    expect(screen.getByText('chat.pending.singleHint')).toBeInTheDocument()
  })

  it('renders batch hint with N items', () => {
    usePendingStore.setState({
      bySession: { s1: [item('a'), item('b'), item('c')] },
    })
    render(<PendingChips sessionId="s1" />)
    expect(screen.getByText('chat.pending.batchHint:3')).toBeInTheDocument()
  })

  it('renders one chip per item', () => {
    usePendingStore.setState({
      bySession: { s1: [item('a'), item('b')] },
    })
    render(<PendingChips sessionId="s1" />)
    expect(screen.getByText(/text-a/)).toBeInTheDocument()
    expect(screen.getByText(/text-b/)).toBeInTheDocument()
  })
})
