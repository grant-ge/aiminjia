import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { McpServerList } from './McpServerList'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: { count?: number }) => {
      if (key === 'settings.mcp.list.tools') {
        return `${options?.count ?? 0} tools`
      }
      return key
    },
  }),
}))

describe('McpServerList', () => {
  it('renders ready and failed states with real status feedback', () => {
    render(
      <McpServerList
        servers={[
          {
            name: 'stdio-ready',
            transportType: 'stdio',
            endpoint: '/usr/local/bin/server',
            state: 'ready',
            registeredToolIds: ['mcp__stdio-ready__echo'],
            lastError: null,
          },
          {
            name: 'http-unsupported',
            transportType: 'http',
            endpoint: 'http://localhost:3000/mcp',
            state: 'failed',
            registeredToolIds: [],
            lastError: 'Unsupported transport: http',
          },
        ]}
        loading={false}
        onConnect={vi.fn(async () => {})}
        onDisconnect={vi.fn(async () => {})}
        onDelete={vi.fn(async () => {})}
        actionLoading={{}}
      />,
    )

    expect(screen.getByText('settings.mcp.list.statusReady')).toBeInTheDocument()
    expect(screen.getByText('settings.mcp.list.statusFailed')).toBeInTheDocument()
    expect(screen.getByText('Unsupported transport: http')).toBeInTheDocument()
    expect(screen.getByText('1 tools')).toBeInTheDocument()
  })
})
