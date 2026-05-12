import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, it, expect, vi } from 'vitest'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (k: string) => k }),
}))

import { PendingChip } from './PendingChip'
import type { PendingItem } from '@/types/pending'

const baseItem: PendingItem = {
  id: 'p-1',
  source: 'app',
  text: 'hello world',
  senderNick: null,
  attachments: [],
  receivedAt: '2026-05-11T03:21:00Z',
}

describe('PendingChip', () => {
  it('renders text content', () => {
    render(<PendingChip item={baseItem} onRemove={() => {}} />)
    expect(screen.getByText(/hello world/)).toBeInTheDocument()
  })

  it('renders sender prefix when senderNick is set', () => {
    const item = { ...baseItem, senderNick: '张三' }
    render(<PendingChip item={item} onRemove={() => {}} />)
    expect(screen.getByText(/张三:/)).toBeInTheDocument()
  })

  it('shows attachment icon when attachments present', () => {
    const item: PendingItem = {
      ...baseItem,
      attachments: [{ id: 'a-1', filePath: '/tmp/x.png' }],
    }
    render(<PendingChip item={item} onRemove={() => {}} />)
    expect(screen.getByTestId('pending-chip-attachment-icon')).toBeInTheDocument()
  })

  it('calls onRemove when × clicked', () => {
    const onRemove = vi.fn()
    render(<PendingChip item={baseItem} onRemove={onRemove} />)
    fireEvent.click(screen.getByLabelText('chat.pending.removeAria'))
    expect(onRemove).toHaveBeenCalledTimes(1)
  })
})
