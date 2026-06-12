import '@testing-library/jest-dom'
import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
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
    expect(chip).toHaveTextContent('技术支持 · 小工')
    expect(screen.getByTestId('chat-avatar')).toBeInTheDocument()
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

  it('renders expert team identity with avatar and suppresses duplicate source chip', () => {
    render(
      <ChatTopBar
        title="专家团: 招聘评审团"
        kind="expertTeam"
        sourceLabel="招聘评审团"
        expertTeam={{
          avatar: <span data-testid="expert-team-avatar" />,
          name: '招聘评审团',
          tagline: '岗位画像 / 候选人评审 / 面试设计',
        }}
      />,
    )

    expect(screen.getByTestId('chat-topbar-expert-team')).toHaveTextContent('专家团 · 招聘评审团')
    expect(screen.getByTestId('expert-team-avatar')).toBeInTheDocument()
    expect(screen.queryByTestId('chat-source-label')).not.toBeInTheDocument()
  })

  it('renders more menu items through AppDropdown', async () => {
    const onSelect = vi.fn()

    render(
      <ChatTopBar
        title="新对话"
        moreMenuItems={[
          {
            id: 'copy-id',
            label: '复制对话 ID',
            onSelect: () => onSelect(),
          },
        ]}
      />,
    )

    await userEvent.click(screen.getByRole('button', { name: '更多' }))
    await userEvent.click(await screen.findByRole('menuitem', { name: '复制对话 ID' }))

    expect(onSelect).toHaveBeenCalledTimes(1)
  })
})
