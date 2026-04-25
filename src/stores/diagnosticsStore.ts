import { create } from 'zustand'
import type { DiagnosticEvent } from '@/lib/diagnostics'

export const MAX_DIAGNOSTIC_EVENTS = 5000

interface DiagnosticsState {
  events: DiagnosticEvent[]
  appendDiagnostic: (event: DiagnosticEvent) => void
  clearDiagnostics: () => void
  getByRunId: (runId: string) => DiagnosticEvent[]
  getByConversationId: (conversationId: string) => DiagnosticEvent[]
}

export const useDiagnosticsStore = create<DiagnosticsState>((set, get) => ({
  events: [],
  appendDiagnostic: (event) =>
    set((state) => {
      const next = [...state.events, event]
      return {
        events: next.length > MAX_DIAGNOSTIC_EVENTS
          ? next.slice(-MAX_DIAGNOSTIC_EVENTS)
          : next,
      }
    }),
  clearDiagnostics: () => set({ events: [] }),
  getByRunId: (runId) => get().events.filter((event) => event.runId === runId),
  getByConversationId: (conversationId) =>
    get().events.filter((event) => event.conversationId === conversationId),
}))
