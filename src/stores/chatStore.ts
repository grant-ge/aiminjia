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

export type ChatState = SessionState & StreamingState

export const useChatStore = create<ChatState>()((set, get) => ({
  ...createSessionSlice<ChatState>(set, get),
  ...createStreamingSlice<ChatState>(set, get),
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
