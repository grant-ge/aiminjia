import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import '@testing-library/jest-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const workspaceState = vi.hoisted(() => ({
  current: null as null | { id: string; rootPath: string; displayName: string },
}))

const tauriMock = vi.hoisted(() => ({
  pickLocalDirectory: vi.fn<(options?: { defaultPath?: string; title?: string }) => Promise<string | null>>(async () => '/tmp/reports'),
  authorizeLocalDirectory: vi.fn(async (path: string) => {
    workspaceState.current = {
      id: 'aw-1',
      rootPath: path,
      displayName: path.split('/').filter(Boolean).pop() ?? path,
    }
    return workspaceState.current
  }),
  getAuthorizedWorkspace: vi.fn(async () => workspaceState.current),
  revokeAuthorizedWorkspace: vi.fn(async () => {
    workspaceState.current = null
  }),
}))

vi.mock('@/lib/tauri', () => tauriMock)

import { WorkspaceAuthPanel } from './WorkspaceAuthPanel'

describe('WorkspaceAuthPanel', () => {
  beforeEach(() => {
    workspaceState.current = null
    tauriMock.pickLocalDirectory.mockClear()
    tauriMock.authorizeLocalDirectory.mockClear()
    tauriMock.getAuthorizedWorkspace.mockClear()
    tauriMock.revokeAuthorizedWorkspace.mockClear()
  })

  it('loads the current authorized workspace for the active session', async () => {
    workspaceState.current = {
      id: 'aw-existing',
      rootPath: '/tmp/existing',
      displayName: 'existing',
    }

    render(<WorkspaceAuthPanel sessionId="session-1" />)

    await waitFor(() => {
      expect(tauriMock.getAuthorizedWorkspace).toHaveBeenCalledWith('session-1')
    })

    expect(await screen.findByText('existing')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '撤销授权' })).toBeInTheDocument()
  })

  it('authorizes a selected directory and refreshes the visible state', async () => {
    render(<WorkspaceAuthPanel sessionId="session-2" />)

    const button = await screen.findByRole('button', { name: '选择工作目录' })
    fireEvent.click(button)

    await waitFor(() => {
      expect(tauriMock.pickLocalDirectory).toHaveBeenCalledWith({
        defaultPath: undefined,
        title: '选择本地工作目录',
      })
      expect(tauriMock.authorizeLocalDirectory).toHaveBeenCalledWith('/tmp/reports', 'session-2')
    })

    expect(await screen.findByText('reports')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '撤销授权' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '重新选择目录' })).toBeInTheDocument()
  })

  it('revokes the current directory and falls back to the empty state', async () => {
    workspaceState.current = {
      id: 'aw-existing',
      rootPath: '/tmp/existing',
      displayName: 'existing',
    }

    render(<WorkspaceAuthPanel sessionId="session-3" />)

    const revokeButton = await screen.findByRole('button', { name: '撤销授权' })
    fireEvent.click(revokeButton)

    await waitFor(() => {
      expect(tauriMock.revokeAuthorizedWorkspace).toHaveBeenCalledWith('session-3')
    })

    expect(await screen.findByRole('button', { name: '选择工作目录' })).toBeInTheDocument()
  })

  it('shows a fallback hint when the native directory picker returns no selection', async () => {
    tauriMock.pickLocalDirectory.mockImplementationOnce(async () => null)

    render(<WorkspaceAuthPanel sessionId="session-4" />)

    fireEvent.click(await screen.findByRole('button', { name: '选择工作目录' }))

    expect(await screen.findByText('未选择目录。若系统目录选择器无法确认，可直接在下方粘贴本地目录路径后授权。')).toBeInTheDocument()
    expect(tauriMock.authorizeLocalDirectory).not.toHaveBeenCalled()
  })

  it('authorizes a manually entered path when directory picker is unavailable', async () => {
    render(<WorkspaceAuthPanel sessionId="session-5" />)

    const manualAuthorizeButton = await screen.findByRole('button', { name: '手动授权' })

    fireEvent.change(
      screen.getByPlaceholderText('/Users/you/Documents/project'),
      { target: { value: '/Users/a20250311/Documents/skills' } },
    )
    fireEvent.click(manualAuthorizeButton)

    await waitFor(() => {
      expect(tauriMock.authorizeLocalDirectory).toHaveBeenCalledWith(
        '/Users/a20250311/Documents/skills',
        'session-5',
      )
    })

    expect(await screen.findByText('skills')).toBeInTheDocument()
  })
})
