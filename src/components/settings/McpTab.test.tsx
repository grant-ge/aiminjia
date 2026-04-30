import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useNotificationStore } from '@/stores/notificationStore'
import { McpTab } from './McpTab'

const tauriMock = vi.hoisted(() => ({
  addMcpServer: vi.fn(),
  connectMcpServer: vi.fn(),
  disconnectMcpServer: vi.fn(),
  listMcpServers: vi.fn(),
  removeMcpServer: vi.fn(),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

vi.mock('@tauri-apps/plugin-dialog', () => ({
  ask: vi.fn(async () => false),
}))

vi.mock('@/lib/tauri', () => tauriMock)

beforeEach(() => {
  tauriMock.addMcpServer.mockReset()
  tauriMock.connectMcpServer.mockReset()
  tauriMock.disconnectMcpServer.mockReset()
  tauriMock.listMcpServers.mockReset()
  tauriMock.removeMcpServer.mockReset()
  useNotificationStore.setState({ notifications: [] })
})

describe('McpTab', () => {
  it('shows an error toast when connect returns a failed server status', async () => {
    tauriMock.listMcpServers
      .mockResolvedValueOnce([
        {
          name: 'demo-server',
          transportType: 'http',
          endpoint: 'http://localhost:3000/mcp',
          state: 'configured',
          registeredToolIds: [],
          lastError: null,
        },
      ])
      .mockResolvedValueOnce([
        {
          name: 'demo-server',
          transportType: 'http',
          endpoint: 'http://localhost:3000/mcp',
          state: 'failed',
          registeredToolIds: [],
          lastError: 'Unsupported transport: http',
        },
      ])

    tauriMock.connectMcpServer.mockResolvedValue({
      name: 'demo-server',
      transportType: 'http',
      endpoint: 'http://localhost:3000/mcp',
      state: 'failed',
      registeredToolIds: [],
      lastError: 'Unsupported transport: http',
    })

    render(<McpTab />)

    await screen.findByText('demo-server')
    fireEvent.click(screen.getByRole('button', { name: 'settings.mcp.list.connect' }))

    await waitFor(() => {
      const notifications = useNotificationStore.getState().notifications
      expect(notifications).toHaveLength(1)
      expect(notifications[0].level).toBe('error')
      expect(notifications[0].title).toBe('settings.mcp.connectFailed')
      expect(notifications[0].message).toContain('Unsupported transport: http')
    })
  })
})
