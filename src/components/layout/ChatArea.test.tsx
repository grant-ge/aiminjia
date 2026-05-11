import '@testing-library/jest-dom'
import { act, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { useChatStore } from '@/stores/chatStore'
import { ChatArea } from './ChatArea'

describe('ChatArea', () => {
  let resizeCallback: ResizeObserverCallback | null = null
  let observeMock: ReturnType<typeof vi.fn>
  let disconnectMock: ReturnType<typeof vi.fn>

  beforeEach(() => {
    HTMLElement.prototype.scrollTo = vi.fn()
    observeMock = vi.fn()
    disconnectMock = vi.fn()
    resizeCallback = null
    class MockResizeObserver {
      constructor(callback: ResizeObserverCallback) {
        resizeCallback = callback
      }

      observe = observeMock
      disconnect = disconnectMock
    }
    vi.stubGlobal('ResizeObserver', MockResizeObserver)
    useChatStore.setState({ activeConversationId: null, messages: [], isStreaming: false })
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('uses a flex scroll region and matches the composer horizontal gutter', () => {
    render(<ChatArea />)

    const scrollRegion = screen.getByTestId('chat-scroll-region')
    expect(scrollRegion).toHaveClass('flex-1')
    expect(scrollRegion).toHaveClass('overflow-y-auto')
    expect(scrollRegion).toHaveClass('[scrollbar-gutter:stable_both-edges]')
    expect(scrollRegion).not.toHaveClass('absolute')
    expect(scrollRegion).not.toHaveStyle({ bottom: '144px' })

    const gutter = scrollRegion.firstElementChild
    expect(gutter).toHaveClass('px-6')
    expect(gutter).toHaveClass('[scrollbar-gutter:stable_both-edges]')
    expect(gutter?.firstElementChild).toHaveClass('w-full')
    expect(gutter?.firstElementChild).toHaveClass('max-w-[736px]')
  })

  it('keeps the scroll pinned to bottom when rendered content grows after image preview loads', () => {
    useChatStore.setState({
      activeConversationId: 'conv-images',
      messages: [{ id: 'm1', role: 'user', content: { text: '![chart](file:///tmp/chart.png)' }, timestamp: 'now' }],
      isStreaming: false,
    })

    render(<ChatArea />)

    const scrollRegion = screen.getByTestId('chat-scroll-region')
    Object.defineProperty(scrollRegion, 'scrollHeight', { configurable: true, value: 1200 })
    Object.defineProperty(scrollRegion, 'clientHeight', { configurable: true, value: 400 })
    scrollRegion.scrollTop = 800

    expect(observeMock).toHaveBeenCalled()
    expect(resizeCallback).not.toBeNull()

    Object.defineProperty(scrollRegion, 'scrollHeight', { configurable: true, value: 1500 })
    act(() => {
      resizeCallback?.([], {} as ResizeObserver)
    })

    expect(scrollRegion.scrollTop).toBe(1500)
  })

  it('does not auto-scroll resized content when the user has scrolled up', () => {
    useChatStore.setState({
      activeConversationId: 'conv-images',
      messages: [{ id: 'm1', role: 'user', content: { text: '![chart](file:///tmp/chart.png)' }, timestamp: 'now' }],
      isStreaming: false,
    })

    render(<ChatArea />)

    const scrollRegion = screen.getByTestId('chat-scroll-region')
    Object.defineProperty(scrollRegion, 'scrollHeight', { configurable: true, value: 1200 })
    Object.defineProperty(scrollRegion, 'clientHeight', { configurable: true, value: 400 })
    scrollRegion.scrollTop = 500
    fireEvent.scroll(scrollRegion)

    Object.defineProperty(scrollRegion, 'scrollHeight', { configurable: true, value: 1500 })
    act(() => {
      resizeCallback?.([], {} as ResizeObserver)
    })

    expect(scrollRegion.scrollTop).toBe(500)
  })

  it('shows an icon button while scrolled up and jumps to bottom when clicked', () => {
    useChatStore.setState({
      activeConversationId: 'conv-scroll',
      messages: [{ id: 'm1', role: 'assistant', content: { text: 'hello' }, timestamp: 'now' }],
      isStreaming: false,
    })

    render(<ChatArea />)

    const scrollRegion = screen.getByTestId('chat-scroll-region')
    Object.defineProperty(scrollRegion, 'scrollHeight', { configurable: true, value: 1200 })
    Object.defineProperty(scrollRegion, 'clientHeight', { configurable: true, value: 400 })
    scrollRegion.scrollTop = 500
    fireEvent.scroll(scrollRegion)

    const jumpButton = screen.getByRole('button', { name: '回到底部' })
    expect(jumpButton).toBeInTheDocument()

    vi.mocked(HTMLElement.prototype.scrollTo).mockClear()
    fireEvent.click(jumpButton)

    expect(HTMLElement.prototype.scrollTo).toHaveBeenCalledWith({ top: 1200, behavior: 'smooth' })
    expect(screen.queryByRole('button', { name: '回到底部' })).not.toBeInTheDocument()
  })

})
