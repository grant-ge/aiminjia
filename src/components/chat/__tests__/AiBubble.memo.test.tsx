import '@testing-library/jest-dom'
import { render } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import type { Message } from '@/types/message'

const renderSpy = vi.fn()

vi.mock('@/components/chat-scene/AssistantMarkdown', () => ({
  AssistantMarkdown: (p: { text: string }) => {
    renderSpy(p.text)
    return <div data-testid="md-stub">{p.text}</div>
  },
}))

vi.mock('@/lib/tauri', () => ({
  sendMessage: vi.fn(),
  openGeneratedFile: vi.fn(),
  revealFileInFolder: vi.fn(),
  getSubagentTranscript: vi.fn(),
}))

vi.mock('@/stores/chatStore', () => ({
  useChatStore: vi.fn(
    (selector: (state: { activeConversationId: string | null }) => unknown) =>
      selector({ activeConversationId: 'conv-1' }),
  ),
}))

vi.mock('@/stores/notificationStore', () => ({
  useNotificationStore: {
    getState: () => ({ push: vi.fn() }),
  },
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (
      key: string,
      fallbackOrOptions?: string | { defaultValue?: string },
    ) => {
      if (typeof fallbackOrOptions === 'string') return fallbackOrOptions
      return fallbackOrOptions?.defaultValue ?? key
    },
  }),
  initReactI18next: { type: '3rdParty', init: () => {} },
}))

import { AiBubble } from '../AiBubble'

function makeMsg(id: string, text: string): Message {
  return {
    id,
    conversationId: 'conv-1',
    role: 'assistant',
    createdAt: '2026-05-15T00:00:00Z',
    content: { text },
  }
}

describe('AiBubble — React.memo', () => {
  it('相同 message 引用 + 相同 isStreaming → 不重渲', () => {
    renderSpy.mockClear()
    const msg = makeMsg('m1', 'hello')

    const { rerender } = render(<AiBubble message={msg} />)
    expect(renderSpy).toHaveBeenCalledTimes(1)

    rerender(<AiBubble message={msg} />)
    expect(renderSpy).toHaveBeenCalledTimes(1)
  })

  it('不同 message 对象引用 → 重渲', () => {
    renderSpy.mockClear()
    const m1 = makeMsg('m1', 'hello')
    const m2 = makeMsg('m1', 'hello') // 内容相同但是新对象

    const { rerender } = render(<AiBubble message={m1} />)
    expect(renderSpy).toHaveBeenCalledTimes(1)

    rerender(<AiBubble message={m2} />)
    expect(renderSpy).toHaveBeenCalledTimes(2)
  })

  it('isStreaming 变化 → 重渲', () => {
    renderSpy.mockClear()
    const msg = makeMsg('m1', 'hello')

    const { rerender } = render(<AiBubble message={msg} isStreaming={false} />)
    expect(renderSpy).toHaveBeenCalledTimes(1)

    rerender(<AiBubble message={msg} isStreaming={true} />)
    expect(renderSpy).toHaveBeenCalledTimes(2)
  })

  // Contract：同一 message 对象被原地 mutate（store 之外）→ memo 浅比较跳过
  // 这是为了把"store 必须 immutable"的契约钉死：如果有人开始原地改 content，
  // 这条测试会暴露——bubble 不刷新，提醒看到 stale 内容的开发者去查 store 路径，
  // 而不是怀疑 memo 出了问题。
  it('同 message 引用被原地 mutate → 不重渲（强制 store 走 immutable）', () => {
    renderSpy.mockClear()
    const msg = makeMsg('m1', 'hello')

    const { rerender } = render(<AiBubble message={msg} />)
    expect(renderSpy).toHaveBeenCalledTimes(1)

    msg.content.text = 'mutated in place'
    rerender(<AiBubble message={msg} />)
    expect(renderSpy).toHaveBeenCalledTimes(1)
  })
})
