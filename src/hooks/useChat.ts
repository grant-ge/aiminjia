/**
 * useChat — High-level chat actions connecting the Zustand store to
 * the Tauri IPC layer.
 *
 * Provides conversation CRUD, message sending, and streaming control.
 *
 * IMPORTANT: All callbacks use useChatStore.getState() to read the latest
 * state inside the callback, rather than capturing the `store` snapshot
 * from render time. This keeps dependencies stable ([]) and avoids
 * infinite re-render loops.
 */
import { useCallback, useRef } from 'react'
import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { useAuthStore } from '@/stores/authStore'
import { useUiStore } from '@/stores/uiStore'
import { useOfflineSendWarning } from './useOfflineSendWarning'
import i18n from '@/i18n'
import { recordDiagnostic, recordDiagnosticError } from '@/lib/diagnostics'
import {
  sendMessage,
  stopStreaming,
  getMessages,
  getTasks,
  createConversation,
  deleteConversation,
  getConversations,
  isAgentBusy as isAgentBusyIpc,
  renameConversation as tauriRenameConversation,
  archiveConversation as tauriArchiveConversation,
  setConversationPinned as tauriSetConversationPinned,
  getActiveTurnStage,
  type ChatAttachmentPayload,
  type SkillCommandPayload,
} from '@/lib/tauri'
import type { Conversation, Message } from '@/types/message'

/** Maximum concurrent conversations allowed (must match backend). */
const MAX_CONCURRENT_AGENTS = 99

/** Generate a unique ID without requiring the `uuid` package. */
function generateId(): string {
  return crypto.randomUUID?.() ?? `${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 9)}`
}

/** File info passed from chat input UI to sendUserMessage. */
export interface PendingFileInfo extends ChatAttachmentPayload {}

export interface PendingSkillCommand extends SkillCommandPayload {
  id: string
}

/**
 * Hook that exposes every chat-related action the UI needs.
 *
 * All functions use stable `useCallback(fn, [])` — they read fresh state
 * via `useChatStore.getState()` inside the callback body.
 */
export function useChat() {
  // Subscribe to state slices for re-rendering.
  // NOTE: streamingContent is intentionally NOT subscribed here.
  // Only MessageList.tsx (which renders StreamingBubble) subscribes to it
  // directly from the store. Subscribing here would force ALL useChat()
  // consumers (Sidebar, ChatBottomArea, App) to re-render on every streaming
  // delta token, saturating the JS main thread and freezing the UI.
  const conversations = useChatStore((s) => s.conversations)
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const messages = useChatStore((s) => s.messages)
  const isStreaming = useChatStore((s) => s.isStreaming)
  const { warnIfOffline } = useOfflineSendWarning()
  const switchVersionRef = useRef(0)

  const syncBusyConversations = useCallback(async (): Promise<Set<string>> => {
    try {
      const busyIds = await isAgentBusyIpc()
      useChatStore.getState().setBusyConversations(busyIds)
      if (busyIds.length > 0) {
        console.log('[useChat] Agent is busy with conversations:', busyIds)
      }
      return new Set(busyIds)
    } catch (err) {
      console.error('[useChat] isAgentBusy IPC failed:', err)
      return new Set(useChatStore.getState().busyConversations)
    }
  }, [])

  /**
   * Create a brand-new conversation and make it active.
   */
  const createNewConversation = useCallback(async () => {
    const store = useChatStore.getState()
    const optimisticId = generateId()
    const now = new Date().toISOString()
    recordDiagnostic({ event: 'conversation.create.started', conversationId: optimisticId })

    const conversation: Conversation = {
      id: optimisticId,
      title: '新对话',
      createdAt: now,
      updatedAt: now,
      isArchived: false,
    }

    // Optimistic store update
    store.setConversations([conversation, ...store.conversations])
    store.setMessages([])

    try {
      const backendId = await createConversation()
      console.log('[useChat] createConversation OK, backendId:', backendId)
      recordDiagnostic({
        event: 'conversation.create.completed',
        ok: true,
        conversationId: backendId ?? optimisticId,
      })

      // Replace optimistic ID with the backend-generated ID
      if (backendId && backendId !== optimisticId) {
        const current = useChatStore.getState()
        current.setConversations(
          current.conversations.map((c) =>
            c.id === optimisticId ? { ...c, id: backendId } : c,
          ),
        )
        useUiStore.getState().setRoute({ kind: 'chat', conversationId: backendId })
        useUiStore.getState().setSidebarTab('project')
        return backendId
      }
    } catch (err) {
      console.error('[useChat] createConversation IPC failed:', err)
      recordDiagnosticError('conversation.create.failed', err, { conversationId: optimisticId })
      // Rollback
      const current = useChatStore.getState()
      current.setConversations(current.conversations.filter((c) => c.id !== optimisticId))
      useUiStore.getState().setRoute({ kind: 'home' })
    }

    useUiStore.getState().setRoute({ kind: 'chat', conversationId: optimisticId })
    useUiStore.getState().setSidebarTab('project')
    return optimisticId
  }, [])

  /**
   * Delete a conversation by id.
   */
  const removeConversation = useCallback(async (id: string) => {
    console.log('[useChat] deleteConversation called, id:', id)
    recordDiagnostic({ event: 'conversation.delete.started', conversationId: id })
    const store = useChatStore.getState()

    store.setConversations(store.conversations.filter((c) => c.id !== id))

    if (store.activeConversationId === id) {
      useUiStore.getState().setRoute({ kind: 'home' })
      store.setMessages([])
    }

    // Clean up per-conversation streaming state and busy tracking to prevent memory leaks
    store.deleteConversationStreamState(id)
    store.removeBusyConversation(id)

    try {
      await deleteConversation(id)
      console.log('[useChat] deleteConversation IPC succeeded')
      recordDiagnostic({ event: 'conversation.delete.completed', ok: true, conversationId: id })
    } catch (err) {
      console.error('[useChat] deleteConversation IPC failed:', err)
      recordDiagnosticError('conversation.delete.failed', err, { conversationId: id })
      // Rollback: reload conversations from backend
      try {
        const raw = await getConversations()
        const convs: Conversation[] = raw
          .map((c) => ({
            id: (c.id as string) ?? '',
            title: (c.title as string) ?? '新对话',
            createdAt: (c.createdAt as string) ?? new Date().toISOString(),
            updatedAt: (c.updatedAt as string) ?? new Date().toISOString(),
            isArchived: (c.isArchived as boolean) ?? false,
            kind: (c.kind as Conversation['kind']) ?? undefined,
            workspaceName: (c.workspaceName as string | undefined) ?? undefined,
            isPinned: (c.isPinned as boolean) ?? false,
          }))
          .filter((c) => c.kind !== 'im')
        useChatStore.getState().setConversations(convs)
      } catch {
        // If re-fetch also fails, nothing more we can do
      }
    }
  }, [])

  /**
   * Switch the active conversation and load its messages from the backend.
   */
  const switchConversation = useCallback(async (id: string) => {
    console.log('[useChat] switchConversation, id:', id)
    recordDiagnostic({ event: 'conversation.switch.started', conversationId: id })
    const loadVersion = ++switchVersionRef.current
    const store = useChatStore.getState()
    // Keep messages already belonging to THIS conversation (covers the
    // home → chat hand-off where HomeTaskComposerCard injects an optimistic
    // user bubble before this effect lands). Drop messages from prior
    // conversations so we don't briefly render stale history.
    store.setMessages(store.messages.filter((m) => m.conversationId === id))
    useUiStore.getState().setRoute({ kind: 'chat', conversationId: id })
    void syncBusyConversations()

    try {
      const [msgs, tasks] = await Promise.all([
        getMessages(id),
        getTasks(id).catch(() => []),
      ])
      if (switchVersionRef.current !== loadVersion) return
      console.log('[useChat] getMessages OK, count:', msgs.length)
      console.log('[useChat] getTasks OK, count:', tasks.length)
      recordDiagnostic({
        event: 'conversation.switch.completed',
        ok: true,
        conversationId: id,
        payload: { messageCount: msgs.length, taskCount: tasks.length },
      })
      // Merge-by-id instead of overwrite. The fetched list is the
      // authoritative server state, but the store may legitimately hold
      // optimistic / in-flight messages (e.g. a user bubble persisted
      // by the home → chat path before backend's T9 commit landed).
      // Race window symptom (pre-fix): user message disappears after
      // sending from the home composer.
      const current = useChatStore.getState().messages
      const fetchedIds = new Set(msgs.map((m) => m.id))
      const echoedClientIds = new Set(
        msgs
          .map((m) => (m as { clientMessageId?: string }).clientMessageId)
          .filter((v): v is string => Boolean(v)),
      )
      const storeOnly = current.filter(
        (m) =>
          m.conversationId === id &&
          !fetchedIds.has(m.id) &&
          !echoedClientIds.has(m.id),
      )
      const merged = storeOnly.length === 0
        ? msgs
        : [...msgs, ...storeOnly].sort((a, b) =>
            a.createdAt < b.createdAt ? -1 : a.createdAt > b.createdAt ? 1 : 0,
          )
      useChatStore.getState().setMessages(merged)
      // 恢复 task 列表到 store
      const store = useChatStore.getState()
      for (const task of tasks) {
        store.upsertConversationTaskState(id, task)
      }
    } catch (err) {
      console.error('[useChat] getMessages IPC failed:', err)
      recordDiagnosticError('conversation.switch.failed', err, { conversationId: id })
    }

    // Spec §5.4: hydrate the persisted turn-stage snapshot so the bubble
    // immediately reflects the in-flight turn's state without waiting for
    // the next 2s heartbeat.  Returns null when no turn is active.
    void getActiveTurnStage(id)
      .then((snapshot) => {
        if (!snapshot) return
        if (switchVersionRef.current !== loadVersion) return
        const store = useChatStore.getState()
        store.setConversationTurnStage(id, snapshot.stage, snapshot.stageStartedAtMs)
        store.addBusyConversation(id)
        recordDiagnostic({
          event: 'turn.stage.hydrated',
          conversationId: id,
          payload: { kind: snapshot.stage.kind, ageMs: Date.now() - snapshot.lastHeartbeatAtMs },
        })
      })
      .catch((err) => {
        console.warn('[useChat] getActiveTurnStage failed:', err)
      })
  }, [syncBusyConversations])

  /**
   * Send a user message in the currently active conversation.
   *
   * @param text   - The user's plain-text input (slash-prefixed text is sent verbatim).
   * @param files  - Optional list of attached file info objects.
   * @param skill  - Optional skill chip selected from the popover. When set,
   *                 the backend persists `skillCommand` metadata on the user
   *                 message which the prompt builder uses to inject SKILL.md
   *                 contents and mark the turn as a skill-driven flow.
   */
  const sendUserMessage = useCallback(async (
    text: string,
    files?: PendingFileInfo[],
    skill?: PendingSkillCommand | null,
  ): Promise<boolean> => {
    let store = useChatStore.getState()
    let conversationId = store.activeConversationId
    console.log('[useChat] sendUserMessage, conversationId:', conversationId, 'text:', text.slice(0, 50))

    if (
      (conversationId && store.busyConversations.has(conversationId))
      || store.busyConversations.size >= MAX_CONCURRENT_AGENTS
    ) {
      await syncBusyConversations()
      store = useChatStore.getState()
      conversationId = store.activeConversationId
    }

    // Note: when THIS conversation is already busy, we used to block here with
    // a "请稍候" toast. Now the backend PendingQueueManager buffers the
    // message and merges it into the next turn after debounce, so we let it
    // through. The UI surfaces the pending state via PendingChips above the
    // composer.
    //
    // Still block if the GLOBAL max concurrent conversations cap is hit, since
    // the queue is per-session and other sessions are already saturated.
    if (store.busyConversations.size >= MAX_CONCURRENT_AGENTS
        && !(conversationId && store.busyConversations.has(conversationId))) {
      useNotificationStore.getState().push({
        level: 'warning',
        title: i18n.t('errors.pleaseWait'),
        message: i18n.t('errors.maxConcurrent', { max: MAX_CONCURRENT_AGENTS }),
        actions: [],
        dismissible: true,
        autoHide: 5,
        context: 'toast',
      })
      return false
    }

    // Auto-create a conversation if none is active
    if (!conversationId) {
      try {
        const backendId = await createConversation()
        console.log('[useChat] Auto-created conversation:', backendId)
        const now = new Date().toISOString()
        store = useChatStore.getState()
        store.setConversations([
          { id: backendId, title: '新对话', createdAt: now, updatedAt: now, isArchived: false },
          ...store.conversations,
        ])
        store.setMessages([])
        useUiStore.getState().setRoute({ kind: 'chat', conversationId: backendId })
        useUiStore.getState().setSidebarTab('project')
        conversationId = backendId
      } catch (err) {
        console.error('[useChat] Failed to auto-create conversation:', err)
        return false
      }
    }

    const messageId = generateId()
    const now = new Date().toISOString()
    const skillCommand = skill
      ? {
        id: skill.id,
        label: skill.label ?? skill.id,
        command: skill.command ?? `/${skill.id}`,
      }
      : null
    recordDiagnostic({
      event: 'chat.submit.started',
      conversationId,
      clientMessageId: messageId,
      payload: { messageLength: text.length, fileCount: files?.length ?? 0 },
    })

    // Build the optimistic user message
    const auth = useAuthStore.getState()
    const userMessage: Message = {
      id: messageId,
      conversationId,
      role: 'user',
      createdAt: now,
      content: {
        text,
        commandText: skillCommand?.command,
        skillCommand: skillCommand ?? undefined,
        files: files?.map((f) => ({
          id: f.id,
          fileName: f.fileName,
          filePath: f.filePath,
          kind: f.kind,
          fileSize: f.fileSize,
          fileType: f.fileType,
          mimeType: f.mimeType,
          status: 'uploaded' as const,
        })),
      },
      sender: {
        name: auth.user?.name || i18n.t('userBubble.me'),
        isLoggedIn: auth.isLoggedIn,
      },
    }

    store = useChatStore.getState()
    // Will the backend queue this message instead of sending it directly?
    // If THIS conversation is currently busy (a turn is in flight), the
    // backend's PendingQueueManager will return Queued and the message will
    // only land in messages.jsonl + UI when drain dispatches the merged turn.
    // Skipping optimistic addMessage here prevents the user from seeing the
    // queued message duplicated as both a chat bubble AND a pending chip.
    const willBeQueued = store.busyConversations.has(conversationId)
    if (!willBeQueued) {
      store.addMessage(userMessage)
      store.setConversationStreaming(conversationId, true)
      store.addBusyConversation(conversationId)
    }

    try {
      console.log('[useChat] Calling sendMessage IPC, attachments:', files, 'willBeQueued:', willBeQueued)
      warnIfOffline()
      await sendMessage(conversationId, text, files, null, messageId, skillCommand)
      console.log('[useChat] sendMessage IPC returned OK')
      recordDiagnostic({
        event: 'chat.submit.completed',
        ok: true,
        conversationId,
        clientMessageId: messageId,
      })
      return true
    } catch (err) {
      console.error('[useChat] sendMessage IPC failed:', err)
      recordDiagnosticError('chat.submit.failed', err, { conversationId, clientMessageId: messageId })
      const s = useChatStore.getState()
      if (!willBeQueued) {
        s.removeMessage(messageId)
        s.clearConversationStreamState(conversationId)
        s.removeBusyConversation(conversationId)
      }
      // Show error toast so user knows the message failed
      useNotificationStore.getState().push({
        level: 'error',
        title: i18n.t('errors.sendFailed'),
        message: String(err) || i18n.t('errors.sendFailedDesc'),
        actions: [],
        dismissible: true,
        autoHide: 8,
        context: 'toast',
      })
      return false
    }
  }, [syncBusyConversations])

  /**
   * Stop the streaming response for the active conversation.
   */
  const stopCurrentStream = useCallback(() => {
    console.log('[useChat] stopCurrentStream')
    const store = useChatStore.getState()
    const convId = store.activeConversationId
    if (convId) {
      recordDiagnostic({ event: 'streaming.stop.requested', conversationId: convId })
      store.clearConversationStreamState(convId)
      stopStreaming(convId).catch((err) => {
        console.error('[useChat] stopStreaming IPC failed:', err)
      })
    }
  }, [])

  /**
   * Load the initial list of conversations from the backend.
   * Also syncs the busy state for crash recovery.
   */
  const loadConversations = useCallback(async () => {
    console.log('[useChat] loadConversations')
    try {
      const raw = await getConversations()
      const convs: Conversation[] = raw
        .map((c) => ({
          id: (c.id as string) ?? '',
          title: (c.title as string) ?? '新对话',
          createdAt: (c.createdAt as string) ?? new Date().toISOString(),
          updatedAt: (c.updatedAt as string) ?? new Date().toISOString(),
          isArchived: (c.isArchived as boolean) ?? false,
          kind: (c.kind as Conversation['kind']) ?? undefined,
          workspaceName: (c.workspaceName as string | undefined) ?? undefined,
          isPinned: (c.isPinned as boolean) ?? false,
        }))
        // Project sidebar only shows app-side conversations; IM-origin
        // chats are surfaced through the channel page (`channelStore`).
        .filter((c) => c.kind !== 'im')
      // dev-only diagnostic：侧边栏首次只看到"默认文件夹"或分组数明显偏少时，
      // 看 workspace tally：若 <none> 占比异常高，多半是后端注入前 race（auth scope 未激活）。
      if (import.meta.env.DEV) {
        const wsTally: Record<string, number> = {}
        for (const c of convs) {
          const k = c.workspaceName ?? '<none>'
          wsTally[k] = (wsTally[k] ?? 0) + 1
        }
        console.log('[diag-sidebar] loadConversations count:', convs.length, 'tally:', wsTally)
      } else {
        console.log('[useChat] loadConversations OK, count:', convs.length)
      }
      useChatStore.getState().setConversations(convs)
    } catch (err) {
      console.error('[useChat] getConversations IPC failed:', err)
    }

    // Sync agent busy state from backend (supports multiple concurrent)
    await syncBusyConversations()
  }, [syncBusyConversations])

  /**
   * Rename a conversation title.
   */
  const renameConversation = useCallback(async (id: string, newTitle: string) => {
    const store = useChatStore.getState()
    recordDiagnostic({ event: 'conversation.rename.started', conversationId: id, payload: { titleLength: newTitle.length } })
    store.setConversations(
      store.conversations.map((c) => c.id === id ? { ...c, title: newTitle } : c)
    )
    try {
      await tauriRenameConversation(id, newTitle)
      recordDiagnostic({ event: 'conversation.rename.completed', ok: true, conversationId: id })
    } catch (err) {
      recordDiagnosticError('conversation.rename.failed', err, { conversationId: id })
      throw err
    }
  }, [])

  const archiveConversation = useCallback(async (id: string) => {
    const store = useChatStore.getState()
    recordDiagnostic({ event: 'conversation.archive.started', conversationId: id })
    // 乐观更新：从列表移除
    store.setConversations(store.conversations.filter((c) => c.id !== id))
    // 如果归档的是当前对话，切回 home
    if (store.activeConversationId === id) {
      useUiStore.getState().setRoute({ kind: 'home' })
    }
    try {
      await tauriArchiveConversation(id)
      recordDiagnostic({ event: 'conversation.archive.completed', ok: true, conversationId: id })
    } catch (err) {
      console.error('[useChat] archiveConversation failed:', err)
      recordDiagnosticError('conversation.archive.failed', err, { conversationId: id })
      // 失败则重新加载
      await loadConversations()
    }
  }, [loadConversations])

  const setConversationPinned = useCallback(async (id: string, pinned: boolean) => {
    const store = useChatStore.getState()
    // Optimistic update — the sidebar reorders synchronously, then we reload
    // from disk to pick up the authoritative ordering.
    store.setConversations(
      store.conversations.map((c) => (c.id === id ? { ...c, isPinned: pinned } : c)),
    )
    try {
      await tauriSetConversationPinned(id, pinned)
      await loadConversations()
    } catch (err) {
      console.error('[useChat] setConversationPinned failed:', err)
      // Roll back on failure.
      await loadConversations()
    }
  }, [loadConversations])

  const createConversationFromSkill = useCallback(async (_skillId: string) => {
    const conversationId = await createNewConversation()
    useUiStore.getState().setRoute({ kind: 'chat', conversationId })
    return conversationId
  }, [createNewConversation])

  return {
    // State (subscribed for re-rendering)
    conversations,
    activeConversationId,
    messages,
    isStreaming,

    // Actions (stable references)
    createNewConversation,
    deleteConversation: removeConversation,
    renameConversation,
    archiveConversation,
    setConversationPinned,
    switchConversation,
    createConversationFromSkill,
    sendUserMessage,
    stopCurrentStream,
    loadConversations,
  }
}
