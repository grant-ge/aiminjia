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
import { recordDiagnostic } from '@/lib/diagnostics'
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
import { useUiStore } from './uiStore'

export type ChatState = SessionState & StreamingState

export const useChatStore = create<ChatState>()((set, get) => ({
  ...(() => {
    const sessionSlice = createSessionSlice<ChatState>(set, get)
    return {
      ...sessionSlice,
      setMessages: (messages: SessionState['messages']) => {
        sessionSlice.setMessages(messages)
        recordDiagnostic({
          event: 'store.messages.set',
          payload: { messageCount: messages.length },
        })
      },
      upsertMessage: (message: SessionState['messages'][number]) => {
        sessionSlice.upsertMessage(message)
        recordDiagnostic({
          event: 'store.messages.upsert',
          conversationId: message.conversationId,
          messageId: message.id,
          payload: { role: message.role },
        })
      },
    }
  })(),
  ...(() => {
    const streamingSlice = createStreamingSlice<ChatState>(set, get)
    return {
      ...streamingSlice,
      addBusyConversation: (id: string) => {
        streamingSlice.addBusyConversation(id)
        const next = useChatStore.getState().busyConversations
        recordDiagnostic({
          event: 'store.busy.add',
          conversationId: id,
          payload: { busyCount: next.size },
        })
      },
      removeBusyConversation: (id: string) => {
        streamingSlice.removeBusyConversation(id)
        const next = useChatStore.getState().busyConversations
        recordDiagnostic({
          event: 'store.busy.remove',
          conversationId: id,
          payload: { busyCount: next.size },
        })
      },
      appendConversationStreamingContent: (convId: string, delta: string) => {
        streamingSlice.appendConversationStreamingContent(convId, delta)
        recordDiagnostic({
          event: 'store.streaming.append',
          conversationId: convId,
          payload: { deltaLength: delta.length },
        })
      },
      clearConversationStreamState: (convId: string) => {
        const previous = useChatStore.getState().streamStates[convId]
        streamingSlice.clearConversationStreamState(convId)
        if (previous) {
          recordDiagnostic({
            event: 'store.streaming.clear',
            conversationId: convId,
            payload: {
              hadStreaming: previous.isStreaming,
              hadContent: previous.streamingContent.length > 0,
              hadAgentPhase: previous.agentPhase != null,
            },
          })
        }
      },
      updateConversationToolExecution: (convId: string, toolId: string, update) => {
        streamingSlice.updateConversationToolExecution(convId, toolId, update)
        recordDiagnostic({
          event: 'store.tool_execution.update',
          conversationId: convId,
          toolCallId: toolId,
          payload: {
            status: update.status,
            durationMs: update.durationMs,
          },
        })
      },
    }
  })(),
}))

bindSessionStore(useChatStore)
bindStreamingStore(useChatStore)

function deriveActiveIdFromRoute(route: ReturnType<typeof useUiStore.getState>['route']): string | null {
  if (route.kind === 'chat') return route.conversationId
  if (route.kind === 'channel') return route.sessionId ?? null
  return null
}

// Initialise synchronously on module load so the first render is correct.
useChatStore.setState({ activeConversationId: deriveActiveIdFromRoute(useUiStore.getState().route) })

// Keep chatStore.activeConversationId in sync whenever the route changes.
useUiStore.subscribe((state, prev) => {
  if (state.route === prev.route) return
  const nextId = deriveActiveIdFromRoute(state.route)
  const prevId = deriveActiveIdFromRoute(prev.route)
  if (nextId === prevId) return
  useChatStore.getState().setActiveConversation(nextId)
})

export { useSessionStore, useStreamingStore }
export type {
  AgentPhase,
  ConversationStreamState,
  ConversationTaskState,
  ToolExecution,
}
