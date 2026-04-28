import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, it, expect, beforeEach, vi } from 'vitest'
import { ChatBottomArea } from '@/components/chat-scene/ChatBottomArea'
import { useUiStore } from '@/stores/uiStore'
import { useChatStore } from '@/stores/chatStore'
import { CREATE_SKILL_COMMAND } from '@/data/skill-constants'

// Mock tauri events and hooks
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(),
}))

vi.mock('@/hooks/useSkillComposer', () => ({
  useSkillComposer: () => ({
    showSkillPopover: false,
    setShowSkillPopover: vi.fn(),
    slashMatch: null,
    slashOpen: false,
    handleSkillPick: vi.fn(),
    handleInputChange: vi.fn(),
    handleSlashSelect: vi.fn(),
    handleSlashClose: vi.fn(),
  }),
}))

vi.mock('@/hooks/useFileUpload', () => ({
  useFileUpload: () => ({
    isUploading: false,
    selectAndUploadFiles: vi.fn(),
  }),
}))

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({
    sendUserMessage: vi.fn(),
    isStreaming: false,
    stopCurrentStream: vi.fn(),
  }),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

describe('ChatBottomArea prefill consumption', () => {
  beforeEach(() => {
    useUiStore.setState({ prefillText: null })
    useChatStore.setState({ activeConversationId: 'test-conv-id' })
  })

  it('input is empty when prefillText is null', () => {
    render(<ChatBottomArea />)
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement
    expect(textarea.value).toBe('')
  })

  it('input is prefilled and store is cleared when prefillText is set', () => {
    useUiStore.setState({ prefillText: CREATE_SKILL_COMMAND })
    render(<ChatBottomArea />)
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement
    expect(textarea.value).toBe(CREATE_SKILL_COMMAND)
    expect(useUiStore.getState().prefillText).toBeNull()
  })
})
