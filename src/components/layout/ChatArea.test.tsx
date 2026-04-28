import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useChatStore } from '@/stores/chatStore'
import { ChatArea } from './ChatArea'

describe('ChatArea', () => {
  beforeEach(() => {
    HTMLElement.prototype.scrollTo = vi.fn()
  })

  it('uses a flex scroll region and centers messages at 736px max width', () => {
    useChatStore.setState({ messages: [], isStreaming: false })

    render(<ChatArea />)

    const scrollRegion = screen.getByTestId('chat-scroll-region')
    expect(scrollRegion).toHaveClass('flex-1')
    expect(scrollRegion).toHaveClass('overflow-y-auto')
    expect(scrollRegion).toHaveClass('[scrollbar-gutter:stable_both-edges]')
    expect(scrollRegion).not.toHaveClass('absolute')
    expect(scrollRegion).not.toHaveStyle({ bottom: '144px' })

    expect(scrollRegion.firstElementChild).toHaveClass('w-full')
    expect(scrollRegion.firstElementChild).toHaveClass('max-w-[736px]')
  })
})
