import '@testing-library/jest-dom'
import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, waitFor } from '@testing-library/react'
import { ChatBottomArea } from '../ChatBottomArea'
import { useUiStore } from '@/stores/uiStore'

vi.mock('@tiptap/react', async (importOriginal) => {
  const mod = await importOriginal<typeof import('@tiptap/react')>()
  return { ...mod, ReactNodeViewRenderer: () => () => ({}) }
})

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ sendUserMessage: vi.fn(), isStreaming: false, stopCurrentStream: vi.fn() }),
}))
vi.mock('@/hooks/useChatAttachments', () => ({
  useChatAttachments: () => ({
    isPickingAttachments: false,
    pickAttachments: vi.fn().mockResolvedValue([]),
    saveClipboardImage: vi.fn(),
    resolvePastedPaths: vi.fn(),
  }),
}))
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (k: string) => k }) }))

beforeEach(() => {
  useUiStore.setState({ prefillText: '帮我看看销售数据' })
})

describe('ChatBottomArea prefill', () => {
  it('consumes prefill text on mount and shows it in the editor', async () => {
    render(<ChatBottomArea />)
    await waitFor(() => {
      const text = document.querySelector('.ProseMirror')?.textContent ?? ''
      expect(text).toContain('帮我看看销售数据')
    })
    expect(useUiStore.getState().prefillText).toBeNull()
  })
})
