/**
 * Chat store — conversation and message state management.
 * Based on tech-architecture.md §3.2
 *
 * Supports multi-conversation concurrency: up to 3 conversations can
 * have active agent loops simultaneously. Streaming state is tracked
 * per-conversation via `streamStates` and `busyConversations`.
 */
import { create } from 'zustand'
import type { Conversation, Message } from '@/types/message'

export interface ToolExecution {
  toolName: string
  toolId: string
  status: 'executing' | 'completed' | 'error'
  summary?: string
}

export interface ConversationStreamState {
  isStreaming: boolean
  streamingContent: string
  toolExecutions: ToolExecution[]
}

interface ChatState {
  // Conversation list
  conversations: Conversation[]
  activeConversationId: string | null

  // Current conversation messages
  messages: Message[]

  // Multi-conversation concurrency state
  busyConversations: Set<string>
  streamStates: Record<string, ConversationStreamState>

  // --- Legacy compatibility getters (derived from per-conversation state) ---
  // These are computed from activeConversationId + streamStates for
  // components that haven't been migrated yet.
  isStreaming: boolean
  streamingContent: string
  toolExecutions: ToolExecution[]

  // Actions — conversations
  setConversations: (conversations: Conversation[]) => void
  setActiveConversation: (id: string | null) => void
  setMessages: (messages: Message[]) => void
  addMessage: (message: Message) => void
  updateMessage: (id: string, updates: Partial<Message>) => void

  // Actions — multi-conversation busy state
  addBusyConversation: (id: string) => void
  removeBusyConversation: (id: string) => void
  setBusyConversations: (ids: string[]) => void

  // Actions — per-conversation streaming
  setConversationStreaming: (convId: string, isStreaming: boolean) => void
  appendConversationStreamingContent: (convId: string, delta: string) => void
  clearConversationStreamState: (convId: string) => void
  resetConversationStreamContent: (convId: string) => void
  deleteConversationStreamState: (convId: string) => void
  addConversationToolExecution: (convId: string, exec: ToolExecution) => void
  updateConversationToolExecution: (convId: string, toolId: string, update: Partial<ToolExecution>) => void

  // Legacy actions (delegate to per-conversation with activeConversationId)
  setStreaming: (isStreaming: boolean) => void
  setStreamingContent: (content: string) => void
  appendStreamingContent: (delta: string) => void
  setAgentBusy: (conversationId: string | null) => void
  addToolExecution: (execution: ToolExecution) => void
  updateToolExecution: (toolId: string, updates: Partial<ToolExecution>) => void
  clearToolExecutions: () => void
}

/** Get or create a stream state entry for a conversation. */
function getStreamState(states: Record<string, ConversationStreamState>, convId: string): ConversationStreamState {
  return states[convId] ?? { isStreaming: false, streamingContent: '', toolExecutions: [] }
}

/** Derive legacy scalar fields from active conversation's stream state. */
function deriveLegacy(activeId: string | null, streamStates: Record<string, ConversationStreamState>) {
  if (!activeId) {
    return { isStreaming: false, streamingContent: '', toolExecutions: [] as ToolExecution[] }
  }
  const s = streamStates[activeId]
  return {
    isStreaming: s?.isStreaming ?? false,
    streamingContent: s?.streamingContent ?? '',
    toolExecutions: s?.toolExecutions ?? [],
  }
}

export const useChatStore = create<ChatState>((set, get) => ({
  conversations: [],
  activeConversationId: null,
  messages: [],
  busyConversations: new Set(),
  streamStates: {},

  // Legacy derived fields (initial values)
  isStreaming: false,
  streamingContent: '',
  toolExecutions: [],

  setConversations: (conversations) => set({ conversations }),

  setActiveConversation: (id) => {
    const { streamStates } = get()
    const legacy = deriveLegacy(id, streamStates)
    set({ activeConversationId: id, ...legacy })
  },

  setMessages: (messages) => set({ messages }),

  addMessage: (message) =>
    set((state) => ({ messages: [...state.messages, message] })),

  updateMessage: (id, updates) =>
    set((state) => ({
      messages: state.messages.map((m) =>
        m.id === id ? { ...m, ...updates } : m,
      ),
    })),

  // --- Multi-conversation busy state ---

  addBusyConversation: (id) =>
    set((state) => {
      const next = new Set(state.busyConversations)
      next.add(id)
      return { busyConversations: next }
    }),

  removeBusyConversation: (id) =>
    set((state) => {
      const next = new Set(state.busyConversations)
      next.delete(id)
      return { busyConversations: next }
    }),

  setBusyConversations: (ids) =>
    set({ busyConversations: new Set(ids) }),

  // --- Per-conversation streaming actions ---

  setConversationStreaming: (convId, isStreaming) =>
    set((state) => {
      const prev = getStreamState(state.streamStates, convId)
      const streamStates = {
        ...state.streamStates,
        [convId]: { ...prev, isStreaming },
      }
      const legacy = deriveLegacy(state.activeConversationId, streamStates)
      return { streamStates, ...legacy }
    }),

  appendConversationStreamingContent: (convId, delta) =>
    set((state) => {
      const prev = getStreamState(state.streamStates, convId)
      const streamStates = {
        ...state.streamStates,
        [convId]: { ...prev, streamingContent: prev.streamingContent + delta },
      }
      const legacy = deriveLegacy(state.activeConversationId, streamStates)
      return { streamStates, ...legacy }
    }),

  clearConversationStreamState: (convId) =>
    set((state) => {
      const prev = state.streamStates[convId]
      if (!prev) {
        return {} // Nothing to clear
      }
      // Reset streaming state but preserve toolExecutions so that late-arriving
      // tool:completed events don't create orphan entries.
      const streamStates = {
        ...state.streamStates,
        [convId]: { isStreaming: false, streamingContent: '', toolExecutions: prev.toolExecutions },
      }
      const legacy = deriveLegacy(state.activeConversationId, streamStates)
      return { streamStates, ...legacy }
    }),

  /** Reset streaming content for a new analysis step (auto-advance).
   *  Clears streamingContent and toolExecutions but keeps isStreaming=true
   *  so the StreamingBubble remains visible for the next step's deltas. */
  resetConversationStreamContent: (convId) =>
    set((state) => {
      const streamStates = {
        ...state.streamStates,
        [convId]: { isStreaming: true, streamingContent: '', toolExecutions: [] },
      }
      const legacy = deriveLegacy(state.activeConversationId, streamStates)
      return { streamStates, ...legacy }
    }),

  /** Fully remove a conversation's stream state (for conversation deletion). */
  deleteConversationStreamState: (convId: string) =>
    set((state) => {
      const { [convId]: _, ...rest } = state.streamStates
      const legacy = deriveLegacy(state.activeConversationId, rest)
      return { streamStates: rest, ...legacy }
    }),

  addConversationToolExecution: (convId, exec) =>
    set((state) => {
      const prev = getStreamState(state.streamStates, convId)
      const streamStates = {
        ...state.streamStates,
        [convId]: { ...prev, toolExecutions: [...prev.toolExecutions, exec] },
      }
      const legacy = deriveLegacy(state.activeConversationId, streamStates)
      return { streamStates, ...legacy }
    }),

  updateConversationToolExecution: (convId, toolId, update) =>
    set((state) => {
      const prev = getStreamState(state.streamStates, convId)
      const streamStates = {
        ...state.streamStates,
        [convId]: {
          ...prev,
          toolExecutions: prev.toolExecutions.map((t) =>
            t.toolId === toolId ? { ...t, ...update } : t,
          ),
        },
      }
      const legacy = deriveLegacy(state.activeConversationId, streamStates)
      return { streamStates, ...legacy }
    }),

  // --- Legacy actions (delegate to per-conversation) ---

  setStreaming: (isStreaming) => {
    const { activeConversationId } = get()
    if (activeConversationId) {
      get().setConversationStreaming(activeConversationId, isStreaming)
    }
  },

  setStreamingContent: (content) => {
    const { activeConversationId, streamStates } = get()
    if (activeConversationId) {
      const prev = getStreamState(streamStates, activeConversationId)
      const next = {
        ...streamStates,
        [activeConversationId]: { ...prev, streamingContent: content },
      }
      const legacy = deriveLegacy(activeConversationId, next)
      set({ streamStates: next, ...legacy })
    }
  },

  appendStreamingContent: (delta) => {
    const { activeConversationId } = get()
    if (activeConversationId) {
      get().appendConversationStreamingContent(activeConversationId, delta)
    }
  },

  setAgentBusy: (conversationId) => {
    // Legacy: single conversation busy. Now delegates to Set-based tracking.
    if (conversationId) {
      get().addBusyConversation(conversationId)
    }
    // If null, do nothing — removeBusyConversation should be called with specific ID
  },

  addToolExecution: (execution) => {
    const { activeConversationId } = get()
    if (activeConversationId) {
      get().addConversationToolExecution(activeConversationId, execution)
    }
  },

  updateToolExecution: (toolId, updates) => {
    const { activeConversationId } = get()
    if (activeConversationId) {
      get().updateConversationToolExecution(activeConversationId, toolId, updates)
    }
  },

  clearToolExecutions: () => {
    const { activeConversationId } = get()
    if (activeConversationId) {
      get().clearConversationStreamState(activeConversationId)
    }
  },
}))
