import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

vi.mock('@/lib/tauri', () => ({
  getConversationModelOverride: vi.fn(),
  setConversationModelOverride: vi.fn(),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, fallback?: { defaultValue?: string }) =>
      fallback?.defaultValue ?? key,
  }),
}))

import {
  getConversationModelOverride,
  setConversationModelOverride,
} from '@/lib/tauri'
import { ModelOverrideSelector } from './ModelOverrideSelector'

describe('ModelOverrideSelector', () => {
  it('loads current override and exposes global option', async () => {
    vi.mocked(getConversationModelOverride).mockResolvedValueOnce('claude')

    render(<ModelOverrideSelector conversationId="conv-1" />)

    expect(await screen.findByDisplayValue('claude')).toBeInTheDocument()
    expect(screen.getByRole('option', { name: '使用全局设置' })).toBeInTheDocument()
  })

  it('persists selected model', async () => {
    vi.mocked(getConversationModelOverride).mockResolvedValueOnce(null)
    vi.mocked(setConversationModelOverride).mockResolvedValueOnce(undefined)

    render(<ModelOverrideSelector conversationId="conv-2" />)

    const select = await screen.findByRole('combobox')
    fireEvent.change(select, { target: { value: 'openai' } })

    await waitFor(() => {
      expect(setConversationModelOverride).toHaveBeenCalledWith('conv-2', 'openai')
    })
  })
})
