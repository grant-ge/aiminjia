import { beforeEach, describe, expect, it, vi } from 'vitest'

const tauriMock = vi.hoisted(() => {
  const runtimeHealth = {
    bundleVersion: 'managed',
    node: { version: 'managed', path: '/tmp/node' },
    npm: null,
    npx: null,
    python: { version: 'managed', path: '/tmp/python' },
    uv: null,
    uvx: null,
  }

  return {
    runtimeHealth,
    getRuntimeHealth: vi.fn().mockResolvedValue(runtimeHealth),
    ensureRuntime: vi.fn().mockResolvedValue(runtimeHealth),
    reinstallRuntime: vi.fn().mockResolvedValue(runtimeHealth),
    cancelRuntimeOperation: vi.fn().mockResolvedValue(true),
    cleanupOldRuntimeVersions: vi.fn().mockResolvedValue({ removedVersions: ['old'], keptVersions: ['current'] }),
  }

  it('stores runtime operation progress and can cancel current operation', async () => {
    useRuntimeStore.getState().applyOperationProgress({
      operationId: 'op-1',
      kind: 'ensure',
      phase: 'download',
      downloadedBytes: 5,
      totalBytes: 10,
      percent: 50,
      attempt: 1,
      maxAttempts: 3,
      resumed: true,
      status: 'progress',
    })

    expect(useRuntimeStore.getState().operationId).toBe('op-1')
    expect(useRuntimeStore.getState().phase).toBe('download')
    expect(useRuntimeStore.getState().percent).toBe(50)

    await useRuntimeStore.getState().cancelCurrentOperation()

    expect(tauriMock.cancelRuntimeOperation).toHaveBeenCalledWith('op-1')
    expect(useRuntimeStore.getState().isCancelling).toBe(false)
  })

  it('cleans old runtime versions through tauri api', async () => {
    await expect(useRuntimeStore.getState().cleanupOldVersions(2)).resolves.toEqual({
      removedVersions: ['old'],
      keptVersions: ['current'],
    })

    expect(tauriMock.cleanupOldRuntimeVersions).toHaveBeenCalledWith(2)
  })

})

vi.mock('@/lib/tauri', () => tauriMock)

import { useRuntimeStore } from '@/stores/runtimeStore'

describe('runtimeStore', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useRuntimeStore.setState({
      health: null,
      isLoading: false,
      isEnsuring: false,
      isReinstalling: false,
      error: null,
      operationId: null,
      phase: null,
      downloadedBytes: 0,
      totalBytes: null,
      percent: null,
      attempt: 0,
      maxAttempts: 0,
      resumed: false,
      isCancelling: false,
    })
  })

  it('loadHealth stores runtime health and clears errors', async () => {
    await useRuntimeStore.getState().loadHealth()

    expect(tauriMock.getRuntimeHealth).toHaveBeenCalled()
    expect(useRuntimeStore.getState().health).toEqual(tauriMock.runtimeHealth)
    expect(useRuntimeStore.getState().isLoading).toBe(false)
    expect(useRuntimeStore.getState().error).toBeNull()
  })

  it('ensure stores returned health', async () => {
    await useRuntimeStore.getState().ensure()

    expect(tauriMock.ensureRuntime).toHaveBeenCalled()
    expect(useRuntimeStore.getState().health).toEqual(tauriMock.runtimeHealth)
    expect(useRuntimeStore.getState().isEnsuring).toBe(false)
  })

  it('reinstall stores returned health', async () => {
    await useRuntimeStore.getState().reinstall()

    expect(tauriMock.reinstallRuntime).toHaveBeenCalled()
    expect(useRuntimeStore.getState().health).toEqual(tauriMock.runtimeHealth)
    expect(useRuntimeStore.getState().isReinstalling).toBe(false)
  })

  it('loadHealth captures error message', async () => {
    tauriMock.getRuntimeHealth.mockRejectedValueOnce(new Error('runtime missing'))

    await expect(useRuntimeStore.getState().loadHealth()).rejects.toThrow('runtime missing')

    expect(useRuntimeStore.getState().isLoading).toBe(false)
    expect(useRuntimeStore.getState().error).toBe('runtime missing')
  })

  it('stores runtime operation progress and can cancel current operation', async () => {
    useRuntimeStore.getState().applyOperationProgress({
      operationId: 'op-1',
      kind: 'ensure',
      phase: 'download',
      downloadedBytes: 5,
      totalBytes: 10,
      percent: 50,
      attempt: 1,
      maxAttempts: 3,
      resumed: true,
      status: 'progress',
    })

    expect(useRuntimeStore.getState().operationId).toBe('op-1')
    expect(useRuntimeStore.getState().phase).toBe('download')
    expect(useRuntimeStore.getState().percent).toBe(50)

    await useRuntimeStore.getState().cancelCurrentOperation()

    expect(tauriMock.cancelRuntimeOperation).toHaveBeenCalledWith('op-1')
    expect(useRuntimeStore.getState().isCancelling).toBe(false)
  })

  it('cleans old runtime versions through tauri api', async () => {
    await expect(useRuntimeStore.getState().cleanupOldVersions(2)).resolves.toEqual({
      removedVersions: ['old'],
      keptVersions: ['current'],
    })

    expect(tauriMock.cleanupOldRuntimeVersions).toHaveBeenCalledWith(2)
  })

})
