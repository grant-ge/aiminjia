import type { StoreApi, UseBoundStore } from 'zustand'
import type { PermissionMode, TurnOutcome, TurnStageKind } from '@/lib/tauri'

export interface ToolExecution {
  toolName: string
  toolId: string
  status: 'executing' | 'completed' | 'error'
  summary?: string
  /** Unix ms timestamp when status changed to 'executing'. */
  startedAt?: number
  /** Elapsed ms from executing → completed/error. Set automatically. */
  durationMs?: number
  input?: unknown  // 来自 tool:executing 事件
  output?: string   // 来自 tool:completed 事件
  /**
   * Most recent stdout/stderr tail snapshot from the live `tool:progress`
   * event. Overwritten each tick; null/undefined once `output` is set.
   * Only populated for tools that emit progress (Bash today).
   */
  progressTail?: string
  /** Total bytes the backend has captured so far (cumulative). */
  progressTotalBytes?: number
  /** Wall-clock Date.now() of the last `tool:progress` event. */
  progressUpdatedAt?: number
}

export interface ConversationTaskState {
  taskId: string
  status: string
  runId: string
  subject: string
  description?: string
  activeForm?: string
  owner?: string
  blockedBy?: string[]
  createdAt?: string
}

export interface TurnSummary {
  outcome: TurnOutcome
  totalInputTokens?: number
  totalOutputTokens?: number
  totalCacheCreationInputTokens?: number
  totalCacheReadInputTokens?: number
  totalCostUsd?: number | null
  completedAt: number
}

export interface CompactSummary {
  preTokens: number
  postTokens: number
  tokensSaved: number
  messagesSummarized: number
  completedAt: number
}

export interface PendingAsk {
  conversationId: string
  runId: string
  toolCallId: string
  toolName: string
  message: string
  suggestions: string[] | null
  mode: PermissionMode
  rememberOptions: Array<'session' | 'workspace' | 'user'> | null
  defaultDestination: 'session' | 'workspace' | 'user' | null
}

export interface ConversationStreamState {
  isStreaming: boolean
  streamingContent: string
  toolExecutions: ToolExecution[]
  lastTurnSummary?: TurnSummary
  lastCompactSummary?: CompactSummary
  /**
   * Backend-driven macro-state of the current turn (spec
   * 2026-05-17-turn-stages §3).  null when no stage has arrived yet
   * (feature flag off, pre-PR1 conversations, or before the first
   * `turn:stage` event lands).
   */
  turnStage?: TurnStageKind | null
  /** Unix ms when the current stage was entered (from `turn:stage`). */
  stageStartedAt?: number | null
  /** Wall-clock ms (Date.now) of the last received `turn:heartbeat`. */
  lastHeartbeatAt?: number | null
  /** Wall-clock ms (Date.now) of the first stage event for this turn. */
  turnStartedAt?: number | null
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
  /**
   * 仅清空 streamingContent 字段，不动 isStreaming/busyConversations/toolExecutions。
   * 用于 iter assistant message 持久化时——该 iter 的文本已落到 messages，避免被下一个
   * iter 的 streaming:delta 累积成"text1text2text3"；但 turn 还在进行中，loading 状态
   * 和正在 executing 的工具不能清。
   */
  clearConversationStreamingTextOnly: (convId: string) => void
  resetConversationStreamContent: (convId: string) => void
  deleteConversationStreamState: (convId: string) => void
  addConversationToolExecution: (convId: string, exec: ToolExecution) => void
  updateConversationToolExecution: (convId: string, toolId: string, update: Partial<ToolExecution>) => void
  upsertConversationTaskState: (convId: string, task: ConversationTaskState) => void
  addPendingAsk: (ask: PendingAsk) => void
  removePendingAsk: (toolCallId: string) => void
  clearConversationPendingAsks: (conversationId: string) => void
  setLastTurnSummary: (convId: string, summary: TurnSummary) => void
  setLastCompactSummary: (convId: string, summary: CompactSummary) => void
  setConversationTurnStage: (
    convId: string,
    stage: TurnStageKind,
    stageStartedAtMs: number,
  ) => void
  clearConversationTurnStage: (convId: string) => void
  touchConversationHeartbeat: (convId: string, atMs: number) => void
  setStreaming: (isStreaming: boolean) => void
  setStreamingContent: (content: string) => void
  appendStreamingContent: (delta: string) => void
  setAgentBusy: (conversationId: string | null) => void
  addToolExecution: (execution: ToolExecution) => void
  updateToolExecution: (toolId: string, updates: Partial<ToolExecution>) => void
  clearToolExecutions: () => void
  resetStreaming: () => void
}

interface StreamingSliceBridge {
  activeConversationId: string | null
}

type SetState<T> = StoreApi<T>['setState']
type GetState<T> = StoreApi<T>['getState']
type SliceUpdate<T> = T | Partial<T> | ((state: T) => T | Partial<T>)

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
  requireStreamingStore().setState(
    partial as Parameters<StreamingStoreHook['setState']>[0],
    replace as Parameters<StreamingStoreHook['setState']>[1],
  )
useStreamingStore.subscribe = ((...args: unknown[]) => {
  const store = requireStreamingStore()
  return (store.subscribe as (...args: unknown[]) => unknown)(...args)
}) as StreamingStoreHook['subscribe']

/** Get or create a stream state entry for a conversation. */
export function getStreamState(
  states: Record<string, ConversationStreamState>,
  convId: string,
): ConversationStreamState {
  return states[convId] ?? {
    isStreaming: false,
    streamingContent: '',
    toolExecutions: [],
  }
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
  const apply = (update: SliceUpdate<T>) => set(update)

  return {
    busyConversations: new Set(),
    streamStates: {},
    taskStates: {},
    pendingAsks: new Map(),
    isStreaming: false,
    streamingContent: '',
    toolExecutions: [],

    addBusyConversation: (id) =>
      apply((state) => {
        const next = new Set(state.busyConversations)
        next.add(id)
        return { busyConversations: next } as Partial<T>
      }),

    removeBusyConversation: (id) =>
      apply((state) => {
        const next = new Set(state.busyConversations)
        next.delete(id)
        return { busyConversations: next } as Partial<T>
      }),

    setBusyConversations: (ids) =>
      apply({ busyConversations: new Set(ids) } as Partial<T>),

    setConversationStreaming: (convId, isStreaming) =>
      apply((state) => {
        const previous = getStreamState(state.streamStates, convId)
        const streamStates = {
          ...state.streamStates,
          [convId]: { ...previous, isStreaming },
        }
        const legacy = deriveLegacy(state.activeConversationId, streamStates)
        return { streamStates, ...legacy } as Partial<T>
      }),

    appendConversationStreamingContent: (convId, delta) =>
      apply((state) => {
        const previous = getStreamState(state.streamStates, convId)
        const streamStates = {
          ...state.streamStates,
          [convId]: { ...previous, streamingContent: previous.streamingContent + delta },
        }
        const legacy = deriveLegacy(state.activeConversationId, streamStates)
        return { streamStates, ...legacy } as Partial<T>
      }),

    clearConversationStreamState: (convId) =>
      apply((state) => {
        const previous = state.streamStates[convId]
        if (!previous) {
          return {} as Partial<T>
        }
        if (
          !previous.isStreaming &&
          previous.streamingContent === '' &&
          previous.toolExecutions.length === 0 &&
          !previous.turnStage
        ) {
          return {} as Partial<T>
        }
        const streamStates = {
          ...state.streamStates,
          [convId]: {
            isStreaming: false,
            streamingContent: '',
            toolExecutions: [],
            lastTurnSummary: previous.lastTurnSummary,
            lastCompactSummary: previous.lastCompactSummary,
            turnStage: null,
            stageStartedAt: null,
            lastHeartbeatAt: null,
            turnStartedAt: null,
          },
        }
        const legacy = deriveLegacy(state.activeConversationId, streamStates)
        return { streamStates, ...legacy } as Partial<T>
      }),

    resetConversationStreamContent: (convId) =>
      apply((state) => {
        const streamStates = {
          ...state.streamStates,
          [convId]: { isStreaming: true, streamingContent: '', toolExecutions: [] },
        }
        const legacy = deriveLegacy(state.activeConversationId, streamStates)
        return { streamStates, ...legacy } as Partial<T>
      }),

    clearConversationStreamingTextOnly: (convId) =>
      apply((state) => {
        const previous = state.streamStates[convId]
        if (!previous || previous.streamingContent === '') return {} as Partial<T>
        const streamStates = {
          ...state.streamStates,
          [convId]: { ...previous, streamingContent: '' },
        }
        const legacy = deriveLegacy(state.activeConversationId, streamStates)
        return { streamStates, ...legacy } as Partial<T>
      }),

    deleteConversationStreamState: (convId) =>
      apply((state) => {
        const rest = { ...state.streamStates }
        delete rest[convId]
        const legacy = deriveLegacy(state.activeConversationId, rest)
        return { streamStates: rest, ...legacy } as Partial<T>
      }),

    addConversationToolExecution: (convId, exec) =>
      apply((state) => {
        const previous = getStreamState(state.streamStates, convId)
        const streamStates = {
          ...state.streamStates,
          [convId]: { ...previous, toolExecutions: [...previous.toolExecutions, exec] },
        }
        const legacy = deriveLegacy(state.activeConversationId, streamStates)
        return { streamStates, ...legacy } as Partial<T>
      }),

    updateConversationToolExecution: (convId, toolId, update) =>
      apply((state) => {
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
        return { streamStates, ...legacy } as Partial<T>
      }),

    upsertConversationTaskState: (convId, task) =>
      apply((state) => {
        const existing = state.taskStates[convId] ?? []
        const index = existing.findIndex((value) => value.taskId === task.taskId)
        const updated =
          index >= 0
            ? existing.map((value, currentIndex) =>
                currentIndex === index ? { ...value, ...task } : value,
              )
            : [...existing, task]
        return { taskStates: { ...state.taskStates, [convId]: updated } } as Partial<T>
      }),

    addPendingAsk: (ask) =>
      apply((state) => {
        const next = new Map(state.pendingAsks)
        next.set(ask.toolCallId, ask)
        return { pendingAsks: next } as Partial<T>
      }),

    removePendingAsk: (toolCallId) =>
      apply((state) => {
        if (!state.pendingAsks.has(toolCallId)) {
          return {} as Partial<T>
        }
        const next = new Map(state.pendingAsks)
        next.delete(toolCallId)
        return { pendingAsks: next } as Partial<T>
      }),

    clearConversationPendingAsks: (conversationId) =>
      apply((state) => {
        const next = new Map(state.pendingAsks)
        let changed = false
        for (const [toolCallId, ask] of next) {
          if (ask.conversationId === conversationId) {
            next.delete(toolCallId)
            changed = true
          }
        }
        return changed
          ? ({ pendingAsks: next } as Partial<T>)
          : ({} as Partial<T>)
      }),

    setLastTurnSummary: (convId, summary) =>
      apply((state) => {
        const previous = getStreamState(state.streamStates, convId)
        const streamStates = {
          ...state.streamStates,
          [convId]: { ...previous, lastTurnSummary: summary },
        }
        const legacy = deriveLegacy(state.activeConversationId, streamStates)
        return { streamStates, ...legacy } as Partial<T>
      }),

    setLastCompactSummary: (convId, summary) =>
      apply((state) => {
        const previous = getStreamState(state.streamStates, convId)
        const streamStates = {
          ...state.streamStates,
          [convId]: { ...previous, lastCompactSummary: summary },
        }
        const legacy = deriveLegacy(state.activeConversationId, streamStates)
        return { streamStates, ...legacy } as Partial<T>
      }),

    setConversationTurnStage: (convId, stage, stageStartedAtMs) =>
      apply((state) => {
        const previous = getStreamState(state.streamStates, convId)
        const turnStartedAt = previous.turnStartedAt ?? Date.now()
        const streamStates = {
          ...state.streamStates,
          [convId]: {
            ...previous,
            turnStage: stage,
            stageStartedAt: stageStartedAtMs,
            lastHeartbeatAt: Date.now(),
            turnStartedAt,
          },
        }
        const legacy = deriveLegacy(state.activeConversationId, streamStates)
        return { streamStates, ...legacy } as Partial<T>
      }),

    clearConversationTurnStage: (convId) =>
      apply((state) => {
        const previous = state.streamStates[convId]
        if (!previous?.turnStage) return {} as Partial<T>
        const streamStates = {
          ...state.streamStates,
          [convId]: {
            ...previous,
            turnStage: null,
            stageStartedAt: null,
            lastHeartbeatAt: null,
          },
        }
        const legacy = deriveLegacy(state.activeConversationId, streamStates)
        return { streamStates, ...legacy } as Partial<T>
      }),

    touchConversationHeartbeat: (convId, atMs) =>
      apply((state) => {
        const previous = state.streamStates[convId]
        if (!previous) return {} as Partial<T>
        const streamStates = {
          ...state.streamStates,
          [convId]: { ...previous, lastHeartbeatAt: atMs },
        }
        const legacy = deriveLegacy(state.activeConversationId, streamStates)
        return { streamStates, ...legacy } as Partial<T>
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
        apply({ streamStates: next, ...legacy } as Partial<T>)
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
        get().addConversationToolExecution(activeConversationId, {
          ...execution,
          startedAt: execution.status === 'executing' ? Date.now() : execution.startedAt,
        })
      }
    },

    updateToolExecution: (toolId, updates) => {
      const { activeConversationId } = get()
      if (activeConversationId) {
        if (updates.status === 'completed' || updates.status === 'error') {
          const existing = get().streamStates[activeConversationId]?.toolExecutions
            .find((t) => t.toolId === toolId)
          if (existing?.startedAt && !updates.durationMs) {
            updates = { ...updates, durationMs: Date.now() - existing.startedAt }
          }
        }
        get().updateConversationToolExecution(activeConversationId, toolId, updates)
      }
    },

    clearToolExecutions: () => {
      const { activeConversationId } = get()
      if (activeConversationId) {
        get().clearConversationStreamState(activeConversationId)
      }
    },

    resetStreaming: () =>
      apply({
        busyConversations: new Set(),
        streamStates: {},
        taskStates: {},
        pendingAsks: new Map(),
        isStreaming: false,
        streamingContent: '',
        toolExecutions: [],
      } as unknown as Partial<T>),
  }
}
