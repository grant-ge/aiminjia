import { beforeEach, describe, expect, it, vi } from 'vitest'

const coreMock = vi.hoisted(() => ({
  invoke: vi.fn(),
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: coreMock.invoke,
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}))

import {
  addMcpServer,
  connectMcpServer,
  disconnectMcpServer,
  listMcpServers,
  removeMcpServer,
  type McpServerConfig,
  type McpServerStatus,
} from './tauri'

describe('tauri mcp commands', () => {
  beforeEach(() => {
    coreMock.invoke.mockReset()
  })

  it('lists MCP servers via the expected command', async () => {
    coreMock.invoke.mockResolvedValue([])

    await listMcpServers()

    expect(coreMock.invoke).toHaveBeenCalledWith('list_mcp_servers')
  })

  it('adds an MCP server via the expected command payload', async () => {
    const config: McpServerConfig = {
      name: 'demo-server',
      transportType: 'stdio',
      endpoint: '/usr/local/bin/demo',
      envVars: {
        API_KEY: 'secret',
      },
    }
    coreMock.invoke.mockResolvedValue(undefined)

    await addMcpServer(config)

    expect(coreMock.invoke).toHaveBeenCalledWith('add_mcp_server', {
      config,
    })
  })

  it('removes an MCP server via the expected command payload', async () => {
    coreMock.invoke.mockResolvedValue(undefined)

    await removeMcpServer('demo-server')

    expect(coreMock.invoke).toHaveBeenCalledWith('remove_mcp_server', {
      serverName: 'demo-server',
    })
  })

  it('connects an MCP server via the expected command payload', async () => {
    const status: McpServerStatus = {
      name: 'demo-server',
      transportType: 'stdio',
      endpoint: '/usr/local/bin/demo',
      state: 'ready',
      registeredToolIds: ['mcp__demo__search'],
      lastError: null,
    }
    coreMock.invoke.mockResolvedValue(status)

    await expect(connectMcpServer('demo-server')).resolves.toEqual(status)

    expect(coreMock.invoke).toHaveBeenCalledWith('connect_mcp_server', {
      serverName: 'demo-server',
    })
  })

  it('disconnects an MCP server via the expected command payload', async () => {
    coreMock.invoke.mockResolvedValue(undefined)

    await disconnectMcpServer('demo-server')

    expect(coreMock.invoke).toHaveBeenCalledWith('disconnect_mcp_server', {
      serverName: 'demo-server',
    })
  })
})
