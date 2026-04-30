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

import { approvePermissionRequest, denyPermissionRequest } from './tauri'

describe('tauri permission commands', () => {
  beforeEach(() => {
    coreMock.invoke.mockReset()
  })

  it('approves a permission request with remember destination payload', async () => {
    coreMock.invoke.mockResolvedValue(undefined)

    await approvePermissionRequest(
      'tool-call-1',
      { path: '/tmp/demo' },
      true,
      'workspace',
    )

    expect(coreMock.invoke).toHaveBeenCalledWith('approve_permission_request', {
      toolCallId: 'tool-call-1',
      updatedInput: { path: '/tmp/demo' },
      remember: true,
      destination: 'workspace',
    })
  })

  it('denies a permission request with scoped remember payload', async () => {
    coreMock.invoke.mockResolvedValue(undefined)

    await denyPermissionRequest(
      'tool-call-2',
      'Denied by user',
      true,
      'user',
    )

    expect(coreMock.invoke).toHaveBeenCalledWith('deny_permission_request', {
      toolCallId: 'tool-call-2',
      message: 'Denied by user',
      remember: true,
      destination: 'user',
    })
  })
})
