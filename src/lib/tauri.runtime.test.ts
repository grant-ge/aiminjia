import { beforeEach, describe, expect, it, vi } from 'vitest'

const coreMock = vi.hoisted(() => ({
  invoke: vi.fn(),
}))

const eventMock = vi.hoisted(() => ({
  listen: vi.fn(),
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: coreMock.invoke,
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: eventMock.listen,
}))

import {
  cancelRuntimeOperation,
  cleanupOldRuntimeVersions,
  ensureRuntime,
  getRuntimeHealth,
  onRuntimeOperationProgress,
  reinstallRuntime,
  type RuntimeHealth,
} from './tauri'

describe('tauri runtime commands', () => {
  beforeEach(() => {
    coreMock.invoke.mockReset()
  })

  it('loads runtime health via the expected command', async () => {
    const health: RuntimeHealth = {
      bundleVersion: 'managed',
      node: { version: 'managed', path: '/tmp/node' },
      npm: null,
      npx: null,
      python: null,
      uv: null,
      uvx: null,
    }
    coreMock.invoke.mockResolvedValue(health)

    await expect(getRuntimeHealth()).resolves.toEqual(health)

    expect(coreMock.invoke).toHaveBeenCalledWith('runtime_get_health')
  })

  it('ensures runtime via the expected command', async () => {
    coreMock.invoke.mockResolvedValue({ bundleVersion: 'managed' })

    await ensureRuntime()

    expect(coreMock.invoke).toHaveBeenCalledWith('runtime_ensure')
  })

  it('reinstalls runtime via the expected command', async () => {
    coreMock.invoke.mockResolvedValue({ bundleVersion: 'managed' })

    await reinstallRuntime()

    expect(coreMock.invoke).toHaveBeenCalledWith('runtime_reinstall')
  })

  it('cancels runtime operation via expected command', async () => {
    coreMock.invoke.mockResolvedValue(true)

    await expect(cancelRuntimeOperation('op-1')).resolves.toBe(true)

    expect(coreMock.invoke).toHaveBeenCalledWith('runtime_cancel_operation', { operationId: 'op-1' })
  })

  it('cleans old runtime versions via expected command', async () => {
    coreMock.invoke.mockResolvedValue({ removedVersions: ['old'], keptVersions: ['current'] })

    await expect(cleanupOldRuntimeVersions(2)).resolves.toEqual({
      removedVersions: ['old'],
      keptVersions: ['current'],
    })

    expect(coreMock.invoke).toHaveBeenCalledWith('runtime_cleanup_old_versions', { keepVersions: 2 })
  })

  it('subscribes to runtime operation progress event', async () => {
    const unlisten = vi.fn()
    eventMock.listen.mockResolvedValue(unlisten)
    const handler = vi.fn()

    await expect(onRuntimeOperationProgress(handler)).resolves.toBe(unlisten)

    expect(eventMock.listen).toHaveBeenCalledWith('runtime:operation-progress', expect.any(Function))
  })

})
