import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useNotificationStore } from '@/stores/notificationStore'
import { useChatStore } from '@/stores/chatStore'
import { ArchivedPanel } from '../panels/ArchivedPanel'

const tauriMock = vi.hoisted(() => ({
  deleteConversation: vi.fn(),
  getArchivedConversations: vi.fn(),
  getConversations: vi.fn(),
  restoreConversation: vi.fn(),
}))

vi.mock('@/lib/tauri', () => tauriMock)

beforeEach(() => {
  tauriMock.deleteConversation.mockReset()
  tauriMock.getArchivedConversations.mockReset()
  tauriMock.getConversations.mockReset()
  tauriMock.restoreConversation.mockReset()
  useNotificationStore.setState({ notifications: [] })
  useChatStore.setState({ conversations: [], activeConversationId: null, messages: [] })
})

describe('ArchivedPanel', () => {
  it('restores an archived conversation after confirmation', async () => {
    tauriMock.getArchivedConversations
      .mockResolvedValueOnce([
        { id: 'c1', title: '归档会话', updatedAt: '2026-04-28T00:00:00Z', isArchived: true },
      ])
      .mockResolvedValueOnce([])
    useChatStore.setState({ activeConversationId: 'c-existing' })
    tauriMock.getConversations.mockResolvedValue([
      { id: 'c1', title: '归档会话', createdAt: '2026-04-27T00:00:00Z', updatedAt: '2026-04-28T00:00:00Z', isArchived: false },
    ])
    tauriMock.restoreConversation.mockResolvedValue(undefined)

    render(<ArchivedPanel />)

    await screen.findByText('归档会话')
    fireEvent.click(screen.getByRole('button', { name: '恢复' }))
    expect(screen.getByText('恢复此对话？')).toBeInTheDocument()
    fireEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: '恢复' }))

    await waitFor(() => expect(tauriMock.restoreConversation).toHaveBeenCalledWith('c1'))
    await waitFor(() => expect(screen.getByText('暂无归档记录')).toBeInTheDocument())
    expect(useChatStore.getState().conversations).toEqual([
      { id: 'c1', title: '归档会话', createdAt: '2026-04-27T00:00:00Z', updatedAt: '2026-04-28T00:00:00Z', isArchived: false, workspaceName: undefined },
    ])
    expect(useChatStore.getState().activeConversationId).toBe('c-existing')
    expect(useNotificationStore.getState().notifications[0]).toMatchObject({
      level: 'success',
      title: '恢复成功',
    })
  })

  it('permanently deletes an archived conversation after confirmation', async () => {
    tauriMock.getArchivedConversations
      .mockResolvedValueOnce([
        { id: 'c1', title: '归档会话', updatedAt: '2026-04-28T00:00:00Z', isArchived: true },
      ])
      .mockResolvedValueOnce([])
    tauriMock.deleteConversation.mockResolvedValue(undefined)

    render(<ArchivedPanel />)

    await screen.findByText('归档会话')
    fireEvent.click(screen.getByRole('button', { name: '彻底删除' }))
    expect(screen.getByText('彻底删除此对话？')).toBeInTheDocument()
    fireEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: '确认' }))

    await waitFor(() => expect(tauriMock.deleteConversation).toHaveBeenCalledWith('c1'))
    await waitFor(() => expect(screen.getByText('暂无归档记录')).toBeInTheDocument())
    expect(useNotificationStore.getState().notifications[0]).toMatchObject({
      level: 'success',
      title: '已彻底删除',
    })
  })

  it('shows an error toast when restore fails', async () => {
    tauriMock.getArchivedConversations.mockResolvedValue([
      { id: 'c1', title: '归档会话', updatedAt: '2026-04-28T00:00:00Z', isArchived: true },
    ])
    tauriMock.restoreConversation.mockRejectedValue(new Error('restore failed'))

    render(<ArchivedPanel />)

    await screen.findByText('归档会话')
    fireEvent.click(screen.getByRole('button', { name: '恢复' }))
    fireEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: '恢复' }))

    await waitFor(() => {
      const notifications = useNotificationStore.getState().notifications
      expect(notifications[0]).toMatchObject({
        level: 'error',
        title: '恢复失败',
      })
      expect(notifications[0].message).toContain('restore failed')
    })
  })
  it('shows an error toast and keeps the item when permanent delete fails', async () => {
    tauriMock.getArchivedConversations.mockResolvedValue([
      { id: 'c1', title: '归档会话', updatedAt: '2026-04-28T00:00:00Z', isArchived: true },
    ])
    tauriMock.deleteConversation.mockRejectedValue(new Error('delete failed'))

    render(<ArchivedPanel />)

    await screen.findByText('归档会话')
    fireEvent.click(screen.getByRole('button', { name: '彻底删除' }))
    fireEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: '确认' }))

    await waitFor(() => {
      const notifications = useNotificationStore.getState().notifications
      expect(notifications[0]).toMatchObject({
        level: 'error',
        title: '删除失败',
      })
      expect(notifications[0].message).toContain('delete failed')
    })
    expect(screen.getByText('归档会话')).toBeInTheDocument()
  })

})
