/**
 * TeamChatEvents 单测：验证 SendMessage 五个 variant 的分流逻辑。
 *
 * 主要校验点：
 *   1. `text` + 非空 content → 渲染 MessageBubble，不应触发协议 SystemDivider。
 *   2. `shutdown_request` → SystemDivider，含 ⊙ 与 shutdownRequest i18n key。
 *   3. `shutdown_response` + approve=true → SystemDivider，含 ✓ 与 shutdownApprove。
 *   4. `shutdown_response` + approve=false → SystemDivider，含 ✗ 与 shutdownReject。
 *   5. `text` + 空 content → 回退到 emptyText 兜底（保留现有行为）。
 *
 * 这些测试不依赖真实 i18n 资源——把 `useTranslation` mock 成把 key 与参数序列化，
 * 这样断言可以精确锁定调用的 key。
 */
import '@testing-library/jest-dom'
import { render } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    /**
     * 简单插值实现：把 opts 序列化拼到 key 后面，方便断言里直接 grep。
     * 例：`t('team.chat.protocol.shutdownRequest', { from:'team-lead', to:'pro' })`
     * → `'team.chat.protocol.shutdownRequest|{"from":"team-lead","to":"pro"}'`
     */
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && Object.keys(opts).length > 0 ? `${key}|${JSON.stringify(opts)}` : key,
  }),
}))

// AssistantMarkdown 在 jsdom 下需要走 react-markdown，渲染开销大；这里 stub 成一个
// 直接打印 text 的简单组件，便于断言并加快 vitest。
vi.mock('@/components/chat-scene/AssistantMarkdown', () => ({
  AssistantMarkdown: ({ text }: { text: string }) => <span data-testid="md">{text}</span>,
}))

import { TeamChatEvents } from './TeamChatEvents'
import type { TeamEvent } from '@/types/team'
import type { ExpertTeam } from '@/features/expert-teams/teams'
import { TeamVisualProvider } from './TeamVisualContext'

function textMessage(text: string): TeamEvent {
  return {
    kind: 'send_message',
    ts: '2026-05-15T15:01:00Z',
    from: 'con-debater',
    to: 'team-lead',
    text,
    isError: false,
    toolCallId: '',
    variant: 'text',
  }
}

const remoteHrTeam: ExpertTeam = {
  id: 'performance-compensation',
  name: '薪酬绩效评审团',
  emoji: '⚖️',
  tagline: '绩效校准 / 调薪方案 / 公平性复核',
  examples: [],
  composerPlaceholder: '告诉他们你要评审的绩效或薪酬方案...',
  facilitationStyle: 'rounds',
  experts: [
    {
      name: '薪酬专家',
      agentName: 'compensation-expert',
      avatar: '薪',
      avatarText: '薪',
      persona: '关注薪酬结构、分位对标和内部公平性',
      emoji: '💰',
    },
  ],
}

describe('TeamChatEvents – send_message variant 分流', () => {
  it('variant=text + 非空文本：走 MessageBubble，不出现协议 icon', () => {
    const { container, getByTestId } = render(
      <TeamChatEvents events={[textMessage('AI 不应该取代初级程序员')]} />,
    )
    expect(getByTestId('md').textContent).toBe('AI 不应该取代初级程序员')
    // 协议 icon 不应出现在 text 气泡里。
    expect(container.textContent).not.toMatch(/[⊙≪]/)
    // 也不应触发"（空消息）"兜底 key。
    expect(container.textContent).not.toContain('team.chat.emptyText')
  })

  it('normal message bubbles use a neutral card surface instead of the old colored boxes', () => {
    const { getByTestId } = render(
      <TeamChatEvents events={[textMessage('AI 不应该取代初级程序员')]} />,
    )

    const bubble = getByTestId('md').parentElement
    expect(bubble).toHaveClass('bg-card')
    expect(bubble).toHaveClass('border-border')
    expect(bubble?.className).not.toMatch(/\bbg-(blue|emerald|rose|amber|violet|cyan)-500\/8\b/)
    expect(bubble?.className).not.toContain('bg-primary/10')
  })

  it('renders system events as compact activity rows without horizontal divider lines', () => {
    const events: TeamEvent[] = [
      {
        kind: 'team_create',
        ts: '2026-05-15T15:00:00Z',
        teamName: '市场营销策划团',
      },
      {
        kind: 'agent_stop',
        ts: '2026-05-15T15:01:00Z',
        agentId: 'brand-lead',
        agentName: 'brand-lead',
      },
    ]

    const { container } = render(<TeamChatEvents events={events} />)

    expect(container.textContent).toContain('team.chat.lifecycle.teamCreatedWithName')
    expect(container.textContent).toContain('team.chat.lifecycle.agentLeft')
    expect(container.querySelector('.h-px.bg-border')).not.toBeInTheDocument()
  })

  it('variant=shutdown_request：渲染 ⊙ SystemDivider，含 shutdownRequest key', () => {
    const event: TeamEvent = {
      kind: 'send_message',
      ts: '2026-05-15T15:02:23Z',
      from: 'team-lead',
      to: 'pro-debater',
      text: '',
      isError: false,
      toolCallId: '',
      variant: 'shutdown_request',
    }
    const { container } = render(<TeamChatEvents events={[event]} />)
    expect(container.textContent).toContain('⊙')
    expect(container.textContent).toContain('team.chat.protocol.shutdownRequest')
    // from / to 已通过参数传给 i18n，调用应携带 from 与 to。
    expect(container.textContent).toContain('"from":"Lead"')
    expect(container.textContent).toContain('"to":"pro-debater"')
    // 不应渲染 MessageBubble（不会冒出"（空消息）"兜底 key）。
    expect(container.textContent).not.toContain('team.chat.emptyText')
  })

  it('variant=shutdown_response + approve=true：渲染 ✓ shutdownApprove', () => {
    const event: TeamEvent = {
      kind: 'send_message',
      ts: '2026-05-15T15:02:28Z',
      from: 'con-debater',
      to: 'team-lead',
      text: '',
      isError: false,
      toolCallId: '',
      variant: 'shutdown_response',
      approve: true,
    }
    const { container } = render(<TeamChatEvents events={[event]} />)
    expect(container.textContent).toContain('✓')
    expect(container.textContent).toContain('team.chat.protocol.shutdownApprove')
    expect(container.textContent).toContain('"from":"con-debater"')
  })

  it('variant=shutdown_response + approve=false：渲染 ✗ shutdownReject', () => {
    const event: TeamEvent = {
      kind: 'send_message',
      ts: '2026-05-15T15:02:28Z',
      from: 'con-debater',
      to: 'team-lead',
      text: '',
      isError: false,
      toolCallId: '',
      variant: 'shutdown_response',
      approve: false,
      reason: 'still working',
    }
    const { container } = render(<TeamChatEvents events={[event]} />)
    expect(container.textContent).toContain('✗')
    // 带 reason 时走 `shutdownRejectWithReason` key。
    expect(container.textContent).toContain('team.chat.protocol.shutdownRejectWithReason')
    expect(container.textContent).toContain('"reason":"still working"')
  })

  it('variant=text + 空文本：兜底渲染 emptyText 占位（行为保留）', () => {
    const { container } = render(<TeamChatEvents events={[textMessage('')]} />)
    expect(container.textContent).toContain('team.chat.emptyText')
    // 仍是 MessageBubble，不应误触协议 SystemDivider 的 icon。
    expect(container.textContent).not.toMatch(/[⊙✗≪]/)
  })

  it('renders remote expert display names instead of stable agent ids', () => {
    const events: TeamEvent[] = [
      {
        kind: 'agent_spawn',
        ts: '2026-05-15T15:00:00Z',
        agentId: 'compensation-expert',
        agentName: 'compensation-expert',
      },
      {
        kind: 'send_message',
        ts: '2026-05-15T15:01:00Z',
        from: 'compensation-expert',
        to: 'team-lead',
        text: '建议先看薪酬分位。',
        isError: false,
        toolCallId: '',
        variant: 'text',
      },
    ]
    const { container } = render(
      <TeamVisualProvider value={remoteHrTeam}>
        <TeamChatEvents events={events} />
      </TeamVisualProvider>,
    )
    expect(container.textContent).toContain('薪酬专家')
    expect(container.textContent).not.toContain('compensation-expert')
  })
})
