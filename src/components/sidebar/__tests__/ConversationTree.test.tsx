import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useSettingsStore } from '@/stores/settingsStore'
import { DEFAULT_SETTINGS } from '@/types/settings'
import { ConversationTree } from '../ConversationTree'

vi.mock('@/lib/tauri', async () => {
  const { DEFAULT_SETTINGS } = await import('@/types/settings')
  return {
    getSettings: vi.fn(async () => useSettingsStore.getState()),
    updateSettings: vi.fn(async () => undefined),
    __DEFAULT_SETTINGS: DEFAULT_SETTINGS,
  }
})

describe('ConversationTree', () => {
  beforeEach(() => {
    localStorage.clear()
    useSettingsStore.setState({ ...DEFAULT_SETTINGS, isLoaded: true })
    vi.clearAllMocks()
  })

  it('limits each project to eight conversations until the user expands it', () => {
    const conversations = Array.from({ length: 10 }, (_, index) => ({
      id: `c-${index + 1}`,
      title: `对话 ${index + 1}`,
    }))

    render(
      <ConversationTree
        projects={[{ id: 'project-a', name: '项目 A', conversations }]}
      />,
    )

    expect(screen.getByText('对话 8')).toBeInTheDocument()
    expect(screen.queryByText('对话 9')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: /显示更多/ }))

    expect(screen.getByText('对话 9')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /收起/ })).toBeInTheDocument()
  })

  it('persists collapsed project state through user-scoped settings, not localStorage', async () => {
    const { updateSettings } = await import('@/lib/tauri')
    const onSelectConversation = vi.fn()
    const project = {
      id: 'project-a',
      name: '项目 A',
      conversations: [{ id: 'c-1', title: '对话 1' }],
    }

    const { unmount } = render(
      <ConversationTree
        projects={[project]}
        onSelectConversation={onSelectConversation}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: /项目 A/ }))
    expect(useSettingsStore.getState().uiSidebarCollapsedProjects).toBe('{"project-a":true}')
    await waitFor(() => {
      expect(updateSettings).toHaveBeenCalledWith(
        expect.objectContaining({ uiSidebarCollapsedProjects: '{"project-a":true}' }),
      )
    })
    expect(localStorage.length).toBe(0)
    unmount()

    render(<ConversationTree projects={[project]} />)

    expect(screen.queryByText('对话 1')).not.toBeInTheDocument()
  })
})
