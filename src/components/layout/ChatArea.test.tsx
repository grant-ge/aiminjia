import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { useChatStore } from '@/stores/chatStore'
import { useSettingsStore } from '@/stores/settingsStore'
import { DEFAULT_SETTINGS } from '@/types/settings'
import { ChatArea } from './ChatArea'

describe('ChatArea', () => {
  beforeEach(() => {
    HTMLElement.prototype.scrollTo = vi.fn()
    useChatStore.setState({ activeConversationId: null, messages: [], isStreaming: false })
    useSettingsStore.setState({ ...DEFAULT_SETTINGS, chatWidthMode: 'centered' })
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('uses a flex scroll region and matches the composer horizontal gutter', () => {
    render(<ChatArea />)

    const scrollRegion = screen.getByTestId('chat-scroll-region')
    expect(scrollRegion).toHaveClass('flex-1')
    expect(scrollRegion).toHaveClass('overflow-y-auto')
    expect(scrollRegion).not.toHaveClass('absolute')
    expect(scrollRegion).not.toHaveStyle({ bottom: '144px' })

    const gutter = scrollRegion.firstElementChild
    expect(gutter).toHaveClass('px-8')
    expect(gutter?.firstElementChild).toHaveClass('w-full')
    expect(gutter?.firstElementChild).toHaveClass('mx-auto')
    expect(gutter?.firstElementChild).toHaveClass('max-w-[736px]')
  })

  it('removes the center max width when chat width mode is full', () => {
    useSettingsStore.setState({ chatWidthMode: 'full' })

    render(<ChatArea />)

    const scrollRegion = screen.getByTestId('chat-scroll-region')
    const content = scrollRegion.firstElementChild?.firstElementChild
    expect(content).toHaveClass('w-full')
    expect(content).not.toHaveClass('mx-auto')
    expect(content).not.toHaveClass('max-w-[736px]')
  })

})
