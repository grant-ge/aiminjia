import type { StoreApi, UseBoundStore } from 'zustand'
import type { Conversation, Message } from '@/types/message'

import { deriveLegacy } from './streamingStore'
import type { ConversationStreamState, ToolExecution } from './streamingStore'

export interface SessionState {
  conversations: Conversation[]
  activeConversationId: string | null
  messages: Message[]
  setConversations: (conversations: Conversation[]) => void
  setActiveConversation: (id: string | null) => void
  setMessages: (messages: Message[]) => void
  addMessage: (message: Message) => void
  updateMessage: (id: string, updates: Partial<Message>) => void
}

interface SessionSliceBridge {
  streamStates: Record<string, ConversationStreamState>
  isStreaming: boolean
  streamingContent: string
  toolExecutions: ToolExecution[]
}

type SetState<T> = StoreApi<T>['setState']
type GetState<T> = StoreApi<T>['getState']

type SessionStoreHook = UseBoundStore<StoreApi<SessionState>>

let boundSessionStore: SessionStoreHook | null = null

export function bindSessionStore<T extends SessionState>(
  store: UseBoundStore<StoreApi<T>>,
): void {
  boundSessionStore = store as unknown as SessionStoreHook
}

function requireSessionStore(): SessionStoreHook {
  if (!boundSessionStore) {
    throw new Error('useSessionStore is not bound yet. Import chatStore before using it.')
  }
  return boundSessionStore
}

export const useSessionStore = ((selector?: (state: SessionState) => unknown) => {
  const store = requireSessionStore()
  return selector ? store(selector as never) : store()
}) as SessionStoreHook

useSessionStore.getState = () => requireSessionStore().getState()
useSessionStore.setState = (partial, replace) =>
  requireSessionStore().setState(partial as Parameters<SessionStoreHook['setState']>[0], replace)
useSessionStore.subscribe = ((...args: unknown[]) => {
  const store = requireSessionStore()
  return (store.subscribe as (...args: unknown[]) => unknown)(...args)
}) as SessionStoreHook['subscribe']

export function createSessionSlice<T extends SessionState & SessionSliceBridge>(
  set: SetState<T>,
  get: GetState<T>,
): SessionState {
  return {
    conversations: [],
    activeConversationId: null,
    messages: [],
    setConversations: (conversations) => set({ conversations }),
    setActiveConversation: (id) => {
      const legacy = deriveLegacy(id, get().streamStates)
      set({ activeConversationId: id, ...legacy })
    },
    setMessages: (messages) => set({ messages }),
    addMessage: (message) =>
      set((state) => ({ messages: [...state.messages, message] })),
    updateMessage: (id, updates) =>
      set((state) => ({
        messages: state.messages.map((message) =>
          message.id === id ? { ...message, ...updates } : message,
        ),
      })),
  }
}
