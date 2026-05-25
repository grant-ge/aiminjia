import '@testing-library/jest-dom'
import { render } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useSettingsStore } from '@/stores/settingsStore'
import { DEFAULT_SETTINGS } from '@/types/settings'
import { ChatBottomArea } from '../ChatBottomArea'

vi.mock('@tiptap/react', async (importOriginal) => {
  const mod = await importOriginal<typeof import('@tiptap/react')>()
  return { ...mod, ReactNodeViewRenderer: () => () => ({}) }
})

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({
    sendUserMessage: vi.fn().mockResolvedValue(undefined),
    isStreaming: false,
    stopCurrentStream: vi.fn(),
  }),
}))

vi.mock('@/hooks/useChatAttachments', () => ({
  useChatAttachments: () => ({
    isPickingAttachments: false,
    pickAttachments: vi.fn().mockResolvedValue([]),
    saveClipboardImage: vi.fn(),
    resolvePastedPaths: vi.fn(),
  }),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

beforeEach(() => {
  useSettingsStore.setState({ ...DEFAULT_SETTINGS, chatWidthMode: 'full' })
})

describe('ChatBottomArea width', () => {
  it('uses full width by default', () => {
    const { container } = render(<ChatBottomArea />)
    const shell = container.querySelector('[data-testid="chat-composer-width-shell"]')

    expect(shell).toHaveClass('w-full')
    expect(shell).not.toHaveClass('mx-auto')
    expect(shell).not.toHaveClass('max-w-[736px]')
  })

  it('uses centered width when the setting is centered', () => {
    useSettingsStore.setState({ chatWidthMode: 'centered' })

    const { container } = render(<ChatBottomArea />)
    const shell = container.querySelector('[data-testid="chat-composer-width-shell"]')

    expect(shell).toHaveClass('mx-auto')
    expect(shell).toHaveClass('w-full')
    expect(shell).toHaveClass('max-w-[736px]')
  })
})
