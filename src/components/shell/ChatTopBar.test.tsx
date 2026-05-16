import '@testing-library/jest-dom'
import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { ChatTopBar } from './ChatTopBar'

describe('ChatTopBar — employee identity card', () => {
  it('renders plain title when no employee is provided', () => {
    render(<ChatTopBar title="新对话" />)
    expect(screen.getByText('新对话')).toBeInTheDocument()
    expect(screen.queryByTestId('chat-topbar-employee')).toBeNull()
  })

  it('replaces title with employee chip when employee is provided', () => {
    render(
      <ChatTopBar
        title="派活: 小工"
        employee={{ avatar: '🛠', name: '小工', role: '技术支持' }}
      />,
    )
    const chip = screen.getByTestId('chat-topbar-employee')
    expect(chip).toBeInTheDocument()
    expect(chip).toHaveTextContent('小工')
    expect(chip).toHaveTextContent('技术支持')
    expect(chip).toHaveTextContent('🛠')
  })

  it('invokes onClick when chip is pressed', () => {
    const onClick = vi.fn()
    render(
      <ChatTopBar
        title="派活: 小工"
        employee={{ avatar: '🛠', name: '小工', role: '技术支持', onClick }}
      />,
    )
    fireEvent.click(screen.getByTestId('chat-topbar-employee'))
    expect(onClick).toHaveBeenCalledTimes(1)
  })

  it('chip is disabled when no onClick provided', () => {
    render(
      <ChatTopBar
        title="x"
        employee={{ avatar: '🛠', name: '小工', role: '技术支持' }}
      />,
    )
    const chip = screen.getByTestId('chat-topbar-employee') as HTMLButtonElement
    expect(chip.disabled).toBe(true)
  })
})
