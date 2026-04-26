import { create } from 'zustand'

import {
  cancelRuntimeOperation,
  cleanupOldRuntimeVersions,
  ensureRuntime,
  getRuntimeHealth,
  reinstallRuntime,
  type RuntimeCleanupResult,
  type RuntimeHealth,
  type RuntimeOperationPhase,
  type RuntimeOperationProgressPayload,
} from '@/lib/tauri'

interface RuntimeState {
  health: RuntimeHealth | null
  isLoading: boolean
  isEnsuring: boolean
  isReinstalling: boolean
  error: string | null
  operationId: string | null
  phase: RuntimeOperationPhase | null
  downloadedBytes: number
  totalBytes: number | null
  percent: number | null
  attempt: number
  maxAttempts: number
  resumed: boolean
  isCancelling: boolean
  loadHealth: () => Promise<void>
  ensure: () => Promise<void>
  reinstall: () => Promise<void>
  applyOperationProgress: (payload: RuntimeOperationProgressPayload) => void
  cancelCurrentOperation: () => Promise<void>
  cleanupOldVersions: (keepVersions: number) => Promise<RuntimeCleanupResult>
  clearError: () => void
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export const useRuntimeStore = create<RuntimeState>((set) => ({
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
  async loadHealth() {
    set({ isLoading: true, error: null })
    try {
      const health = await getRuntimeHealth()
      set({ health, isLoading: false, error: null })
    } catch (error) {
      set({ isLoading: false, error: errorMessage(error) })
      throw error
    }
  },
  async ensure() {
    set({ isEnsuring: true, error: null })
    try {
      const health = await ensureRuntime()
      set({ health, isEnsuring: false, error: null })
    } catch (error) {
      set({ isEnsuring: false, error: errorMessage(error) })
      throw error
    }
  },
  async reinstall() {
    set({ isReinstalling: true, error: null })
    try {
      const health = await reinstallRuntime()
      set({ health, isReinstalling: false, error: null })
    } catch (error) {
      set({ isReinstalling: false, error: errorMessage(error) })
      throw error
    }
  },

  applyOperationProgress(payload) {
    set({
      operationId: payload.status === 'completed' || payload.status === 'failed' || payload.status === 'cancelled' ? null : payload.operationId,
      phase: payload.phase,
      downloadedBytes: payload.downloadedBytes ?? 0,
      totalBytes: payload.totalBytes ?? null,
      percent: payload.percent ?? null,
      attempt: payload.attempt,
      maxAttempts: payload.maxAttempts,
      resumed: payload.resumed,
      error: payload.status === 'failed' ? payload.error ?? 'runtime operation failed' : null,
      isEnsuring: payload.status === 'completed' || payload.status === 'failed' || payload.status === 'cancelled' ? false : undefined,
      isReinstalling: payload.status === 'completed' || payload.status === 'failed' || payload.status === 'cancelled' ? false : undefined,
      isCancelling: payload.status === 'cancelled' ? false : undefined,
    })
  },
  async cancelCurrentOperation() {
    const operationId = useRuntimeStore.getState().operationId
    if (!operationId) return
    set({ isCancelling: true })
    try {
      await cancelRuntimeOperation(operationId)
    } finally {
      set({ isCancelling: false })
    }
  },
  async cleanupOldVersions(keepVersions) {
    return cleanupOldRuntimeVersions(keepVersions)
  },
  clearError() {
    set({ error: null })
  },
}))
