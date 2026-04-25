/**
 * Chat store — session/message CRUD + streaming/task state.
 *
 * `useChatStore` remains the single Zustand owner so existing selectors,
 * `getState()`, and `setState()` calls keep working. Session/streaming files
 * now own their slice logic and expose bound views into this store.
 */
import { create } from 'zustand'

import {
  bindSessionStore,
  createSessionSlice,
  type SessionState,
  useSessionStore,
} from './sessionStore'
import {
  bindStreamingStore,
  createStreamingSlice,
  type AgentPhase,
  type ConversationStreamState,
  type ConversationTaskState,
  type StreamingState,
  type ToolExecution,
  useStreamingStore,
} from './streamingStore'

export interface ComposerSkillCommand {
  id: string
  label: string
  command: string
}

export type ChatState = SessionState & StreamingState & {
  selectedSkillCommands: Record<string, ComposerSkillCommand>
  setSelectedSkillCommand: (conversationId: string, command: ComposerSkillCommand | null) => void
  clearSelectedSkillCommand: (conversationId?: string | null) => void
}

export const useChatStore = create<ChatState>()((set, get) => ({
  ...createSessionSlice<ChatState>(set, get),
  ...createStreamingSlice<ChatState>(set, get),
  selectedSkillCommands: {},
  setSelectedSkillCommand: (conversationId, command) => set((state) => {
    const next = { ...state.selectedSkillCommands }
    if (command) {
      next[conversationId] = command
    } else {
      delete next[conversationId]
    }
    return { selectedSkillCommands: next }
  }),
  clearSelectedSkillCommand: (conversationId) => set((state) => {
    if (!conversationId) return { selectedSkillCommands: state.selectedSkillCommands }
    const next = { ...state.selectedSkillCommands }
    delete next[conversationId]
    return { selectedSkillCommands: next }
  }),
}))

bindSessionStore(useChatStore)
bindStreamingStore(useChatStore)

export { useSessionStore, useStreamingStore }
export type {
  AgentPhase,
  ConversationStreamState,
  ConversationTaskState,
  ToolExecution,
}
