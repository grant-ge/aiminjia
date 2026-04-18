import type { StoreApi, UseBoundStore } from 'zustand'

export interface ToolExecution {
  toolName: string
  toolId: string
  status: 'executing' | 'completed' | 'error'
  summary?: string
}

export type AgentPhase = 'think' | 'act' | 'observe'

export interface ConversationTaskState {
  taskId: string
  status: string
  runId: string
}

export interface PendingAsk {
  conversationId: string
  runId: string
  toolCallId: string
  toolName: string
  message: string
  suggestions: string[] | null
}

export interface ConversationStreamState {
  isStreaming: boolean
  streamingContent: string
  toolExecutions: ToolExecution[]
  agentPhase?: AgentPhase
}

export interface StreamingState {
  busyConversations: Set<string>
  streamStates: Record<string, ConversationStreamState>
  taskStates: Record<string, ConversationTaskState[]>
  pendingAsks: Map<string, PendingAsk>
  isStreaming: boolean
  streamingContent: string
  toolExecutions: ToolExecution[]
  addBusyConversation: (id: string) => void
  removeBusyConversation: (id: string) => void
  setBusyConversations: (ids: string[]) => void
  setConversationStreaming: (convId: string, isStreaming: boolean) => void
  appendConversationStreamingContent: (convId: string, delta: string) => void
  clearConversationStreamState: (convId: string) => void
  resetConversationStreamContent: (convId: string) => void
  deleteConversationStreamState: (convId: string) => void
  addConversationToolExecution: (convId: string, exec: ToolExecution) => void
  updateConversationToolExecution: (convId: string, toolId: string, update: Partial<ToolExecution>) => void
  setConversationAgentPhase: (convId: string, phase: AgentPhase | undefined) => void
  upsertConversationTaskState: (convId: string, task: ConversationTaskState) => void
  addPendingAsk: (ask: PendingAsk) => void
  removePendingAsk: (toolCallId: string) => void
  clearConversationPendingAsks: (conversationId: string) => void
  setStreaming: (isStreaming: boolean) => void
  setStreamingContent: (content: string) => void
  appendStreamingContent: (delta: string) => void
  setAgentBusy: (conversationId: string | null) => void
  addToolExecution: (execution: ToolExecution) => void
  updateToolExecution: (toolId: string, updates: Partial<ToolExecution>) => void
  clearToolExecutions: () => void
}

interface StreamingSliceBridge {
  activeConversationId: string | null
}

type SetState<T> = StoreApi<T>['setState']
type GetState<T> = StoreApi<T>['getState']

type StreamingStoreHook = UseBoundStore<StoreApi<StreamingState>>

let boundStreamingStore: StreamingStoreHook | null = null

export function bindStreamingStore<T extends StreamingState>(
  store: UseBoundStore<StoreApi<T>>,
): void {
  boundStreamingStore = store as unknown as StreamingStoreHook
}

function requireStreamingStore(): StreamingStoreHook {
  if (!boundStreamingStore) {
    throw new Error('useStreamingStore is not bound yet. Import chatStore before using it.')
  }
  return boundStreamingStore
}

export const useStreamingStore = ((selector?: (state: StreamingState) => unknown) => {
  const store = requireStreamingStore()
  return selector ? store(selector as never) : store()
}) as StreamingStoreHook

useStreamingStore.getState = () => requireStreamingStore().getState()
useStreamingStore.setState = (partial, replace) =>
  requireStreamingStore().setState(partial as Parameters<StreamingStoreHook['setState']>[0], replace)
useStreamingStore.subscribe = ((...args: unknown[]) => {
  const store = requireStreamingStore()
  return (store.subscribe as (...args: unknown[]) => unknown)(...args)
}) as StreamingStoreHook['subscribe']

/** Get or create a stream state entry for a conversation. */
export function getStreamState(
  states: Record<string, ConversationStreamState>,
  convId: string,
): ConversationStreamState {
  return states[convId] ?? { isStreaming: false, streamingContent: '', toolExecutions: [] }
}

/** Derive legacy scalar fields from active conversation's stream state. */
export function deriveLegacy(
  activeId: string | null,
  streamStates: Record<string, ConversationStreamState>,
) {
  if (!activeId) {
    return { isStreaming: false, streamingContent: '', toolExecutions: [] as ToolExecution[] }
  }
  const streamState = streamStates[activeId]
  return {
    isStreaming: streamState?.isStreaming ?? false,
    streamingContent: streamState?.streamingContent ?? '',
    toolExecutions: streamState?.toolExecutions ?? [],
  }
}

export function createStreamingSlice<T extends StreamingState & StreamingSliceBridge>(
  set: SetState<T>,
  get: GetState<T>,
): StreamingState {
  return {
    busyConversations: new Set(),
    streamStates: {},
    taskStates: {},
    pendingAsks: new Map(),
    isStreaming: false,
    streamingContent: '',
    toolExecutions: [],

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

    setBusyConversations: (ids) => set({ busyConversations: new Set(ids) }),

    setConversationStreaming: (convId, isStreaming) =>
      set((state) => {
        const previous = getStreamState(state.streamStates, convId)
        const streamStates = {
          ...state.streamStates,
          [convId]: { ...previous, isStreaming },
        }
        const legacy = deriveLegacy(state.activeConversationId, streamStates)
        return { streamStates, ...legacy }
      }),

    appendConversationStreamingContent: (convId, delta) =>
      set((state) => {
        const previous = getStreamState(state.streamStates, convId)
        const streamStates = {
          ...state.streamStates,
          [convId]: { ...previous, streamingContent: previous.streamingContent + delta },
        }
        const legacy = deriveLegacy(state.activeConversationId, streamStates)
        return { streamStates, ...legacy }
      }),

    clearConversationStreamState: (convId) =>
      set((state) => {
        const previous = state.streamStates[convId]
        if (!previous) {
          return {}
        }
        if (!previous.isStreaming && previous.streamingContent === '' && !previous.agentPhase) {
          return {}
        }
        const streamStates = {
          ...state.streamStates,
          [convId]: {
            isStreaming: false,
            streamingContent: '',
            toolExecutions: previous.toolExecutions,
            agentPhase: undefined,
          },
        }
        const legacy = deriveLegacy(state.activeConversationId, streamStates)
        return { streamStates, ...legacy }
      }),

    resetConversationStreamContent: (convId) =>
      set((state) => {
        const streamStates = {
          ...state.streamStates,
          [convId]: { isStreaming: true, streamingContent: '', toolExecutions: [] },
        }
        const legacy = deriveLegacy(state.activeConversationId, streamStates)
        return { streamStates, ...legacy }
      }),

    deleteConversationStreamState: (convId) =>
      set((state) => {
        const rest = { ...state.streamStates }
        delete rest[convId]
        const legacy = deriveLegacy(state.activeConversationId, rest)
        return { streamStates: rest, ...legacy }
      }),

    addConversationToolExecution: (convId, exec) =>
      set((state) => {
        const previous = getStreamState(state.streamStates, convId)
        const streamStates = {
          ...state.streamStates,
          [convId]: { ...previous, toolExecutions: [...previous.toolExecutions, exec] },
        }
        const legacy = deriveLegacy(state.activeConversationId, streamStates)
        return { streamStates, ...legacy }
      }),

    updateConversationToolExecution: (convId, toolId, update) =>
      set((state) => {
        const previous = getStreamState(state.streamStates, convId)
        const streamStates = {
          ...state.streamStates,
          [convId]: {
            ...previous,
            toolExecutions: previous.toolExecutions.map((tool) =>
              tool.toolId === toolId ? { ...tool, ...update } : tool,
            ),
          },
        }
        const legacy = deriveLegacy(state.activeConversationId, streamStates)
        return { streamStates, ...legacy }
      }),

    setConversationAgentPhase: (convId, phase) =>
      set((state) => {
        const previous = getStreamState(state.streamStates, convId)
        const streamStates = {
          ...state.streamStates,
          [convId]: { ...previous, agentPhase: phase },
        }
        return { streamStates }
      }),

    upsertConversationTaskState: (convId, task) =>
      set((state) => {
        const existing = state.taskStates[convId] ?? []
        const index = existing.findIndex((value) => value.taskId === task.taskId)
        const updated =
          index >= 0
            ? existing.map((value, currentIndex) =>
                currentIndex === index ? { ...value, ...task } : value,
              )
            : [...existing, task]
        return { taskStates: { ...state.taskStates, [convId]: updated } }
      }),

    addPendingAsk: (ask) =>
      set((state) => {
        const next = new Map(state.pendingAsks)
        next.set(ask.toolCallId, ask)
        return { pendingAsks: next }
      }),

    removePendingAsk: (toolCallId) =>
      set((state) => {
        if (!state.pendingAsks.has(toolCallId)) {
          return {}
        }
        const next = new Map(state.pendingAsks)
        next.delete(toolCallId)
        return { pendingAsks: next }
      }),

    clearConversationPendingAsks: (conversationId) =>
      set((state) => {
        const next = new Map(state.pendingAsks)
        let changed = false
        for (const [toolCallId, ask] of next) {
          if (ask.conversationId === conversationId) {
            next.delete(toolCallId)
            changed = true
          }
        }
        return changed ? { pendingAsks: next } : {}
      }),

    setStreaming: (isStreaming) => {
      const { activeConversationId } = get()
      if (activeConversationId) {
        get().setConversationStreaming(activeConversationId, isStreaming)
      }
    },

    setStreamingContent: (content) => {
      const { activeConversationId, streamStates } = get()
      if (activeConversationId) {
        const previous = getStreamState(streamStates, activeConversationId)
        const next = {
          ...streamStates,
          [activeConversationId]: { ...previous, streamingContent: content },
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
      if (conversationId) {
        get().addBusyConversation(conversationId)
      }
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
  }
}
