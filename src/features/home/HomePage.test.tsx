import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const sendUserMessage = vi.fn(async () => undefined)

// Mock stores that HomePage's children touch
const setRoute = vi.fn()
const consumePrefillText = vi.fn(() => null)
const setPermissionModeForSession = vi.fn()
const setReasoningModeForSession = vi.fn()
const uiState = {
  route: { kind: 'home' },
  setRoute,
  openSettings: vi.fn(),
  consumePrefillText,
  consumePendingSkill: vi.fn(() => null),
  permissionModesBySession: {},
  reasoningModesBySession: {},
  setPermissionModeForSession,
  setReasoningModeForSession,
}

vi.mock('@/stores/uiStore', () => ({
  DRAFT_PERMISSION_SESSION_ID: '__draft__',
  DRAFT_REASONING_SESSION_ID: '__draft_reasoning__',
  useUiStore: Object.assign(
    (sel: (s: unknown) => unknown) => sel(uiState),
    { getState: () => uiState },
  ),
}))

vi.mock('@/stores/brandingStore', () => ({
  useBrandingStore: (sel: (s: unknown) => unknown) =>
    sel({ productName: 'AI 小家', logoUrl: '/app-icon.png', accentColor: '#DBAA22' }),
}))

vi.mock('@/stores/chatStore', () => ({
  useChatStore: (sel: (s: unknown) => unknown) =>
    sel({ messages: [], isStreaming: false, streamingContent: '', activeConversationId: null, streamStates: {}, busyConversations: new Set() }),
}))

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({
    sendUserMessage,
    conversations: [],
    activeConversationId: null,
    switchConversation: vi.fn(),
    createNewConversation: vi.fn(),
    sendMessage: vi.fn(),
    isStreaming: false,
  }),
}))

import { HomePage } from './HomePage'

describe('HomePage', () => {
  beforeEach(() => {
    sendUserMessage.mockClear()
    setRoute.mockClear()
    consumePrefillText.mockClear()
    setPermissionModeForSession.mockClear()
    setReasoningModeForSession.mockClear()
  })

  it('renders mascot title, quick example categories, and composer without secondary CTA', () => {
    render(<HomePage />)
    expect(screen.getAllByText('让每个伙伴，都有一支会办事的 AI 团队').length).toBeGreaterThanOrEqual(1)
    expect(screen.queryByRole('button', { name: /前往技能中心/ })).not.toBeInTheDocument()
    const categoryButtons = screen.getAllByTestId('home-quick-category-scroll')[0].querySelectorAll('button')
    expect(Array.from(categoryButtons).slice(0, 3).map((button) => button.textContent)).toEqual([
      '应用连接',
      'HR 专家',
      '通用助手',
    ])
    expect(screen.getByRole('button', { name: /应用连接/ })).toHaveAttribute('aria-pressed', 'false')
    expect(screen.getByRole('button', { name: /HR 专家/ })).toHaveAttribute('aria-pressed', 'false')
    expect(screen.getByRole('button', { name: /通用助手/ })).toHaveAttribute('aria-pressed', 'false')
    expect(screen.getByRole('button', { name: /日常办公/ })).toHaveAttribute('aria-pressed', 'false')
    expect(screen.getByRole('button', { name: /数据分析/ })).toHaveAttribute('aria-pressed', 'false')
    expect(screen.getByRole('button', { name: /文档处理/ })).toHaveAttribute('aria-pressed', 'false')
    expect(screen.getByRole('button', { name: /代码开发/ })).toHaveAttribute('aria-pressed', 'false')
    expect(screen.getByRole('button', { name: /定时任务/ })).toHaveAttribute('aria-pressed', 'false')
    expect(screen.queryByRole('button', { name: /周计划/ })).not.toBeInTheDocument()
  })

  it('flips from categories to examples and back after choosing an example', () => {
    render(<HomePage />)

    fireEvent.click(screen.getByRole('button', { name: /数据分析/ }))

    expect(screen.queryByRole('button', { name: /日常办公/ })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: /财报分析全流程/ })).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: /财报分析全流程/ }))

    expect(screen.getByRole('button', { name: /数据分析/ })).toHaveAttribute('aria-pressed', 'false')
    expect(screen.queryByRole('button', { name: /财报分析全流程/ })).not.toBeInTheDocument()
  })

  it('shows code development examples with concrete zero-dependency starters', () => {
    render(<HomePage />)

    fireEvent.click(screen.getByRole('button', { name: /代码开发/ }))

    expect(screen.queryByRole('button', { name: /日常办公/ })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: /作品集网站/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /贪吃蛇游戏/ })).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: /作品集网站/ }))

    expect(screen.getByRole('button', { name: /代码开发/ })).toHaveAttribute('aria-pressed', 'false')
    expect(screen.queryByRole('button', { name: /作品集网站/ })).not.toBeInTheDocument()
  })

  it('shows connection, HR expert, and general assistant example groups', () => {
    render(<HomePage />)

    fireEvent.click(screen.getByRole('button', { name: /应用连接/ }))
    expect(screen.getByRole('button', { name: /发送钉钉消息/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /查询入离职/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /查询工资条/ })).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /发送钉钉消息/ }))

    fireEvent.click(screen.getByRole('button', { name: /HR 专家/ }))
    expect(screen.getByRole('button', { name: /分析薪酬公平性/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /盘点人才梯队/ })).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /分析薪酬公平性/ }))

    fireEvent.click(screen.getByRole('button', { name: /通用助手/ }))
    expect(screen.getByRole('button', { name: /生成事件报告/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /观点转 PPT/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /校验 Excel 数据/ })).toBeInTheDocument()
  })

  it('vertically centers the main home content column', () => {
    const { container } = render(<HomePage />)
    const pageWrapper = container.querySelector('.mx-auto.max-w-\\[1280px\\]')
    expect(pageWrapper?.className).toMatch(/justify-center/)
    const contentColumn = Array.from(container.querySelectorAll('div')).find((el) =>
      el.className.includes('w-[760px]'),
    )
    expect(contentColumn?.className).toMatch(/items-start/)
    expect(contentColumn?.querySelector('.flex.w-full.flex-col')?.className).toMatch(/gap-16/)
  })

  it('renders quick examples as single-row horizontal scrollers', () => {
    const { container } = render(<HomePage />)
    const categoryScroll = screen.getByTestId('home-quick-category-scroll')
    expect(categoryScroll.className).toMatch(/overflow-x-auto/)
    expect(categoryScroll.className).toMatch(/overflow-y-hidden/)
    expect(categoryScroll.className).toMatch(/scrollbar/)

    fireEvent.click(screen.getByRole('button', { name: /代码开发/ }))
    const exampleScroll = screen.getByTestId('home-quick-example-scroll')
    expect(exampleScroll.className).toMatch(/overflow-x-auto/)
    expect(exampleScroll.querySelectorAll('button').length).toBeGreaterThan(4)
    expect(container.querySelector('[data-testid="home-quick-example-face"]')?.className).not.toMatch(/flex-wrap/)
  })

  it('uses tenant logoUrl from branding store as mascot', () => {
    const { container } = render(<HomePage />)
    const mascotImg = container.querySelector('[data-testid="home-mascot"] img')
    expect(mascotImg).toHaveAttribute('src', '/app-icon.png')
  })



})
