/**
 * useStreaming — Listens to Tauri streaming events and pipes them
 * into the chat store, routing by conversationId.
 *
 * This hook should be mounted once at a high level (e.g. in the main
 * chat layout) so that streaming events are processed for the lifetime
 * of the application.
 *
 * Events handled:
 *  - streaming:delta  — appends token content to the per-conversation streaming buffer
 *  - streaming:done   — finalises the streamed message for a conversation
 *  - streaming:error  — surfaces the error to the user
 *  - message:updated  — upserts the full message object in the store
 *  - tool:executing   — tracks tool execution state per conversation
 *  - tool:completed   — updates tool execution completion per conversation
 *  - streaming:step-reset — clears content between auto-advancing analysis steps
 *  - agent:idle       — clears busy state for a specific conversation
 *
 * Safety watchdog:
 *  A 200-second inactivity watchdog runs every 10 seconds. If any
 *  conversation has isStreaming=true but received no delta/tool event
 *  for 200 seconds, the streaming state is force-cleared. This prevents
 *  the UI from being permanently stuck due to missed Tauri events.
 *  When a conversation first starts streaming (no activity recorded yet),
 *  the watchdog initializes the timestamp rather than clearing immediately,
 *  giving the full timeout window for the first backend event to arrive.
 *
 * Delta throttling:
 *  Streaming deltas are accumulated in a ref buffer and flushed to the
 *  Zustand store at most once per animation frame (~60fps). This prevents
 *  high-frequency token events (50-100+/s) from saturating the React
 *  render loop and freezing the UI.
 */
import { useEffect, useRef } from 'react'
import { listen } from '@tauri-apps/api/event'
import { useChatStore } from '@/stores/chatStore'
import { useAuthStore } from '@/stores/authStore'
import { useDiagnosticsStore } from '@/stores/diagnosticsStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { recordDiagnostic } from '@/lib/diagnostics'
import i18n from '@/i18n'
import type { Message } from '@/types/message'
import {
  onStreamingDelta,
  onStreamingDone,
  onStreamingError,
  onStreamingRetryReset,
  onMessageUpdated,
  onToolExecuting,
  onToolCompleted,
  onAgentIdle,
  onPermissionAsk,
  onInteractionRequired,
  onInteractionResolved,
  onFileGenerated,
  onTaskStatusChanged,
  onTurnCompleted,
  onTurnStage,
  onTurnHeartbeat,
  onDiagnosticsEvent,
  uploadDiagnosticLogs,
  TAURI_EVENTS,
} from '@/lib/tauri'
import type {
  StreamingDeltaPayload,
  StreamingDonePayload,
  StreamingErrorPayload,
  StreamingRetryResetPayload,
  AgentIdlePayload,
  ToolExecutingPayload,
  PermissionAskPayload,
  InteractionRequiredPayload,
  InteractionResolvedPayload,
  FileGeneratedPayload,
  TaskStatusChangedPayload,
  TurnCompletedPayload,
  TurnStagePayload,
  TurnHeartbeatPayload,
  DiagnosticsEventPayload,
} from '@/lib/tauri'
import { useStreamingStore } from '@/stores/streamingStore'
import type { ConversationTaskState } from '@/stores/streamingStore'
import { useInteractionStore } from '@/stores/interactionStore'
import { useTauriEvent } from './useTauriEvent'

/** How long (ms) before a streaming conversation with no activity is force-cleared.
 *  Set to 200s to accommodate analysis mode step transitions which involve
 *  checkpoint extraction (up to 30s) + LLM cold start + first tool execution.
 *  Must exceed backend's analysis chunk timeout (180s in chat.rs). */
const STALE_STREAM_TIMEOUT_MS = 200_000

/** How often (ms) the watchdog checks for stale streams. */
const WATCHDOG_INTERVAL_MS = 10_000

const autoUploadedFailedToolCalls = new Set<string>()

/** Detect gateway error strings that indicate the currently selected
 *  cloud model is no longer routable (route disabled, model unknown,
 *  protocol mismatch, etc). When matched we resync cloud models from the
 *  server, which falls back to a known-good default in authStore so the
 *  user can resend instead of being stuck.
 *
 *  Substrings are matched case-insensitively against `error || rawError`
 *  from `streaming:error` payload.
 */
const ROUTE_ERROR_HINTS = [
  'no anthropic-protocol route',
  'no_route',
  'no route found',
  'model_not_found',
  'not_found_error',
  'no chat model configured',
] as const

function looksLikeRouteError(text: string | undefined | null): boolean {
  if (!text) return false
  const lower = text.toLowerCase()
  return ROUTE_ERROR_HINTS.some((h) => lower.includes(h))
}

/** Cooldown to prevent a thrashing model list reload if the user
 *  triggers several failing turns in a row. */
const RESYNC_COOLDOWN_MS = 30_000
let lastModelResyncAt = 0

async function resyncCloudModelsOnRouteError(): Promise<void> {
  const now = Date.now()
  if (now - lastModelResyncAt < RESYNC_COOLDOWN_MS) return
  lastModelResyncAt = now
  try {
    const before = useAuthStore.getState().selectedCloudModel
    const after = await useAuthStore.getState().resyncCloudModels()
    if (after && after !== before) {
      useNotificationStore.getState().push({
        level: 'info',
        title: i18n.t('errors.modelRouteUnavailable'),
        message: i18n.t('errors.modelRouteUnavailableDesc', { model: after }),
        actions: [],
        dismissible: true,
        autoHide: 10,
        context: 'toast',
      })
    }
  } catch (err) {
    console.warn('[streaming:error] resync cloud models failed:', err)
  }
}

function extractTaskCreateState(message: Message): ConversationTaskState | null {
  const match = message.toolResult?.content.match(/^Task #(\S+) created successfully: (.+)$/)
  if (!match) return null
  return {
    taskId: match[1],
    status: 'pending',
    runId: message.runId ?? '',
    subject: match[2],
  }
}

function maybeAutoUploadDiagnosticsForToolError(message: Message): void {
  const result = message.toolResult
  if (!result?.isError) return

  const key = `${message.conversationId}:${result.toolCallId}`
  if (autoUploadedFailedToolCalls.has(key)) return
  autoUploadedFailedToolCalls.add(key)

  void uploadDiagnosticLogs().catch((err) => {
    console.warn('[diagnostics:auto-upload] failed after tool error:', err)
  })
}

/**
 * Registers all streaming-related Tauri event listeners.
 *
 * Call this hook once in a top-level component. It does not return
 * anything — all side-effects flow through the Zustand stores.
 */
export function useStreaming() {
  // NOTE: We intentionally do NOT destructure store state here.
  // The Tauri event callbacks below are registered once via useTauriEvent([]),
  // so any captured references would be stale. Instead, we call getState()
  // inside each callback to always access fresh store state.

  // --- Streaming activity tracker (for watchdog) ---
  // Records the last time any streaming event was received per conversation.
  const lastActivityRef = useRef<Record<string, number>>({})

  // --- Delta throttle buffer ---
  // Accumulates streaming deltas per conversation and flushes them to the
  // Zustand store at most once per animation frame (~60fps). Without this,
  // every single token (50-100+/s) triggers a Zustand set() → React re-render
  // cascade, which saturates the JS main thread and freezes the UI.
  const deltaBufferRef = useRef<Record<string, string>>({})
  const rafIdRef = useRef<number | null>(null)

  // --- Streaming timing diagnostics（临时排查"无流式感"问题）---
  // 记录每个对话的：首 delta 时间、上一次 flush 时间、累计 delta 个数/字节数。
  // 用 console.log 直接输出，前端 DevTools Console 可见。
  const streamTimingRef = useRef<Record<string, {
    firstDeltaAt: number
    lastFlushAt: number
    deltaCount: number
    totalBytes: number
    flushCount: number
  }>>({})

  /** Flush accumulated deltas to the store, then clear the buffer. */
  function flushDeltas() {
    rafIdRef.current = null
    const buffer = deltaBufferRef.current
    const keys = Object.keys(buffer)
    if (keys.length === 0) return

    // Take a snapshot and clear the buffer first, so new deltas arriving
    // during the flush go into the next frame.
    deltaBufferRef.current = {}

    for (const convId of keys) {
      const accumulated = buffer[convId]
      if (accumulated) {
        const t = streamTimingRef.current[convId]
        if (t) {
          const now = performance.now()
          const sinceLast = t.lastFlushAt > 0 ? Math.round(now - t.lastFlushAt) : 0
          const sinceFirst = Math.round(now - t.firstDeltaAt)
          t.flushCount += 1
          t.lastFlushAt = now
          console.log(
            '[stream-timing] flush#%d conv=%s bytes=%d sinceLast=%dms sinceFirst=%dms totalBytes=%d totalDeltas=%d',
            t.flushCount, convId, accumulated.length, sinceLast, sinceFirst, t.totalBytes, t.deltaCount,
          )
        }
        recordDiagnostic({
          event: 'streaming.delta.flushed',
          conversationId: convId,
          payload: { deltaLength: accumulated.length },
        })
        useChatStore.getState().appendConversationStreamingContent(convId, accumulated)
      }
    }
  }

  /** Schedule a flush on the next animation frame (if not already scheduled). */
  function scheduleFlush() {
    if (rafIdRef.current === null) {
      rafIdRef.current = requestAnimationFrame(flushDeltas)
    }
  }

  // Clean up any pending rAF on unmount
  useEffect(() => {
    return () => {
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current)
        // Flush remaining deltas synchronously on unmount
        flushDeltas()
      }
    }
  }, [])

  /** Mark a conversation as having recent streaming activity. */
  function touchActivity(conversationId: string) {
    lastActivityRef.current[conversationId] = Date.now()
  }

  /** Flush buffered deltas for a specific conversation synchronously.
   *  Must be called before clearConversationStreamState() to avoid losing
   *  deltas that arrived after the last rAF flush. */
  function flushConversationDeltas(conversationId: string) {
    const buffered = deltaBufferRef.current[conversationId]
    if (buffered) {
      recordDiagnostic({
        event: 'streaming.delta.flushed',
        conversationId,
        payload: { deltaLength: buffered.length, mode: 'sync' },
      })
      useChatStore.getState().appendConversationStreamingContent(conversationId, buffered)
    }
    delete deltaBufferRef.current[conversationId]
  }

  // --- streaming:delta -------------------------------------------------
  useTauriEvent(() =>
    onStreamingDelta(({ conversationId, delta }: StreamingDeltaPayload) => {
      touchActivity(conversationId)
      // 时序统计：首次到达打一条，后续累计计数
      let t = streamTimingRef.current[conversationId]
      if (!t) {
        const now = performance.now()
        t = { firstDeltaAt: now, lastFlushAt: 0, deltaCount: 0, totalBytes: 0, flushCount: 0 }
        streamTimingRef.current[conversationId] = t
        console.log('[stream-timing] FIRST delta conv=%s len=%d', conversationId, delta.length)
      }
      t.deltaCount += 1
      t.totalBytes += delta.length
      recordDiagnostic({
        event: 'streaming.delta.received',
        conversationId,
        payload: { deltaLength: delta.length },
      })
      // Buffer the delta instead of immediately updating the store
      deltaBufferRef.current[conversationId] =
        (deltaBufferRef.current[conversationId] ?? '') + delta
      scheduleFlush()
    }),
  )

  // --- streaming:done --------------------------------------------------
  useTauriEvent(() =>
    onStreamingDone(({ conversationId }: StreamingDonePayload) => {
      const t = streamTimingRef.current[conversationId]
      if (t) {
        const now = performance.now()
        const totalMs = Math.round(now - t.firstDeltaAt)
        console.log(
          '[stream-timing] DONE conv=%s totalDeltas=%d totalBytes=%d flushes=%d totalMs=%dms avgBytesPerFlush=%d',
          conversationId, t.deltaCount, t.totalBytes, t.flushCount, totalMs,
          t.flushCount > 0 ? Math.round(t.totalBytes / t.flushCount) : 0,
        )
        delete streamTimingRef.current[conversationId]
      } else {
        console.log('[stream-timing] DONE conv=%s (no deltas received)', conversationId)
      }
      console.log('[streaming:done] conversationId:', conversationId)
      recordDiagnostic({ event: 'streaming.done.received', conversationId })
      // Flush buffered deltas synchronously before clearing stream state
      flushConversationDeltas(conversationId)
      delete lastActivityRef.current[conversationId]
      const store = useChatStore.getState()
      store.clearConversationStreamState(conversationId)
      store.removeBusyConversation(conversationId)
      useStreamingStore.getState().clearConversationPendingAsks(conversationId)
    }),
  )

  // --- streaming:error -------------------------------------------------
  useTauriEvent(() =>
    onStreamingError(({ conversationId, error, rawError }: StreamingErrorPayload) => {
      console.error('[streaming:error]', conversationId, rawError ?? 'unknown', error)
      recordDiagnostic({
        event: 'streaming.error.received',
        level: 'error',
        ok: false,
        conversationId,
        error,
        payload: { rawError },
      })
      // Flush buffered deltas so partial content is preserved before clearing
      flushConversationDeltas(conversationId)
      delete lastActivityRef.current[conversationId]
      const store = useChatStore.getState()
      store.clearConversationStreamState(conversationId)
      store.removeBusyConversation(conversationId)
      if (conversationId === store.activeConversationId) {
        const lastUserMsg = [...store.messages]
          .reverse()
          .find((m) => m.role === 'user' && m.conversationId === conversationId)
        if (lastUserMsg && !lastUserMsg.id.startsWith('msg-')) {
          store.removeMessage(lastUserMsg.id)
        }
      }

      // Auto-recover from "selected model has no route" errors. The gateway
      // route catalog can change at any time (ops disables a route, a tenant
      // fixed-route flips, etc.); when that happens the desktop client's
      // locally-cached cloud_model goes stale and every send fails until the
      // user manually picks a new model. Resync silently and surface a toast
      // telling the user a switch happened.
      if (looksLikeRouteError(error) || looksLikeRouteError(rawError)) {
        void resyncCloudModelsOnRouteError()
      }

      // Show longer auto-hide for timeout errors (user needs time to read)
      const autoHideSecs = rawError === 'chunk_timeout' || rawError === 'agent_timeout' ? 15 : 8

      const suffix = ''

      useNotificationStore.getState().push({
        level: 'error',
        title: i18n.t('errors.streamingError'),
        message: (error ?? i18n.t('errors.unknownRetry')) + suffix,
        actions: [],
        dismissible: true,
        autoHide: autoHideSecs,
        context: 'toast',
      })
    }),
  )

  // --- message:updated -------------------------------------------------
  useTauriEvent(() =>
    onMessageUpdated((message) => {
      const store = useChatStore.getState()
      const clientMessageId = (message as Message & { clientMessageId?: string }).clientMessageId
      if (message.role === 'user' && clientMessageId && message.conversationId === store.activeConversationId) {
        const optimistic = store.messages.find((m) => m.id === clientMessageId)
        if (optimistic) {
          const idx = store.messages.findIndex((m) => m.id === clientMessageId)
          const updated = [...store.messages]
          const runId = (message as Message & { runId?: string }).runId
          const merged = {
            ...optimistic,
            ...message,
            content: { ...optimistic.content, ...message.content },
          }
          console.debug('[skill-command][message-updated-merge]', {
            traceId: runId ?? clientMessageId,
            conversationId: message.conversationId,
            clientMessageId,
            persistedMessageId: message.id,
            runId,
            optimisticSkillCommand: optimistic.content.skillCommand,
            persistedSkillCommand: message.content.skillCommand,
            mergedSkillCommand: merged.content.skillCommand,
            optimisticCommandText: optimistic.content.commandText,
            persistedCommandText: message.content.commandText,
            mergedCommandText: merged.content.commandText,
          })
          updated[idx] = merged
          store.setMessages(updated)
          return
        }
      }
      // Always process messages for the active conversation.
      // For non-active conversations, the message is already persisted in DB
      // and will be loaded when the user switches to that conversation
      // (switchConversation calls getMessages which reloads from DB).
      if (message.conversationId === store.activeConversationId) {
        const exists = store.messages.some((m) => m.id === message.id)
        if (exists) {
          store.updateMessage(message.id, message)
        } else {
          store.addMessage(message)
        }
      } else {
        // Non-active conversation: message already saved to DB by finish_agent.
        // Log for debugging multi-conversation concurrency.
        console.log(
          '[message:updated] Received for non-active conversation %s (active: %s), will load from DB on switch',
          message.conversationId,
          store.activeConversationId,
        )
      }

      // When we receive a persisted assistant message, clear the streaming
      // state IN THE SAME callback so React batches both updates into one
      // render. This prevents the visual "flash" where StreamingBubble
      // unmounts (streaming:done) before the persisted assistant bubble appears.
      //
      // Skip iteration-only messages: assistant messages that carry toolCalls
      // but no text are mid-turn snapshots (one per tool-call batch). Clearing
      // streaming state here would kill the loading indicator while more tool
      // calls are still in flight.
      const isIterationToolCallsMessage =
        message.role === 'assistant' &&
        !message.content.text &&
        (message.toolCalls?.length ?? 0) > 0
      if (message.role === 'assistant' && !isIterationToolCallsMessage) {
        const streamState = store.streamStates[message.conversationId]
        if (streamState?.isStreaming) {
          console.log('[message:updated] Clearing streaming state for %s (assistant message persisted)', message.conversationId)
          recordDiagnostic({
            event: 'streaming.done.received',
            conversationId: message.conversationId,
            payload: { source: 'assistant.message.persisted' },
          })
          flushConversationDeltas(message.conversationId)
          delete lastActivityRef.current[message.conversationId]
          store.clearConversationStreamState(message.conversationId)
          store.removeBusyConversation(message.conversationId)
        }
      }
    }),
  )

  // --- streaming:retry-reset -------------------------------------------
  useTauriEvent(() =>
    onStreamingRetryReset(({ conversationId }: StreamingRetryResetPayload) => {
      console.log('[streaming:retry-reset]', conversationId)
      recordDiagnostic({ event: 'streaming.retry_reset.received', conversationId })
      delete deltaBufferRef.current[conversationId]
      useChatStore.getState().resetConversationStreamContent(conversationId)
    }),
  )

  // --- tool:executing ---------------------------------------------------
  useTauriEvent(() =>
    onToolExecuting(({ conversationId, toolName, toolId, purpose, input, scope }: ToolExecutingPayload) => {
      // Sub-agent internal tool calls are emitted with scope:'child' purely
      // for downstream visibility (e.g. a future sub-agent detail pane).
      // Filter them out of the parent conversation's tool-execution panel —
      // they would otherwise pile up as never-completing rows.
      if (scope === 'child') {
        return
      }
      console.log('[tool:executing]', conversationId, toolName, toolId)
      touchActivity(conversationId)
      recordDiagnostic({
        event: 'tool.executing.received',
        conversationId,
        toolCallId: toolId,
        payload: { toolName, purpose },
      })
      useChatStore.getState().addConversationToolExecution(conversationId, {
        toolName,
        toolId,
        status: 'executing',
        summary: purpose,
        input,
      })
    }),
  )

  // --- tool:completed ---------------------------------------------------
  useTauriEvent(() =>
    onToolCompleted((message: Message) => {
      // Filter sub-agent completions for the same reason as tool:executing.
      // Backend tags these with `scope:'child'` (not part of the strict
      // Message type, hence the cast).
      if ((message as { scope?: string }).scope === 'child') {
        return
      }
      console.log('[tool:completed]', message.conversationId, message.toolResult?.name)
      touchActivity(message.conversationId)
      recordDiagnostic({
        event: 'tool.completed.received',
        conversationId: message.conversationId,
        toolCallId: message.toolResult?.toolCallId,
        ok: !message.toolResult?.isError,
        payload: { toolName: message.toolResult?.name },
      })
      const store = useChatStore.getState()
      if (message.conversationId === store.activeConversationId) {
        store.upsertMessage(message)
      }
      if (message.toolResult) {
        maybeAutoUploadDiagnosticsForToolError(message)
        if (message.toolResult.name === 'TaskCreate' && !message.toolResult.isError) {
          const task = extractTaskCreateState(message)
          if (task) {
            store.upsertConversationTaskState(message.conversationId, task)
          }
        }
        store.updateConversationToolExecution(
          message.conversationId,
          message.toolResult.toolCallId,
          {
            status: message.toolResult.isError ? 'error' : 'completed',
            durationMs: message.toolResult.durationMs,
            output: message.toolResult.content,
          },
        )
      }
    }),
  )

  useTauriEvent(() =>
    listen<{ conversationId: string; reason?: string }>(
      TAURI_EVENTS.STOP_PREVENTED_CONTINUATION,
      (event) => {
        const { conversationId } = event.payload
        console.warn('[stop:prevented-continuation]', conversationId)
        recordDiagnostic({ event: 'streaming.stop_prevented.received', conversationId })
        useChatStore.getState().clearConversationStreamState(conversationId)
        useChatStore.getState().removeBusyConversation(conversationId)
      },
    ),
  )

  // --- agent:idle --------------------------------------------------------
  useTauriEvent(() =>
    onAgentIdle(({ conversationId, scope, agentId }: AgentIdlePayload) => {
      // Determine effective scope: explicit scope wins; if absent but agentId
      // is present, this is a child/background agent (legacy compat).
      const effectiveScope = scope ?? (agentId ? 'child' : 'primary')

      // Child/background agent idle should not clear parent conversation state
      if (effectiveScope === 'child') {
        console.log('[agent:idle] child agent idle for conversationId:', conversationId, '— skipping parent state clear')
        // Auto-resume the parent: a background sub-agent finished, so its
        // <task-notification> is now sitting in the queue. Trigger a sentinel
        // send_message so the parent's chat_turn_driver drains the notification
        // and the LLM continues.
        const parentBusy = useStreamingStore.getState().busyConversations.has(conversationId)
        console.log('[agent:idle] auto-resume parent: conversationId=', conversationId, 'parentBusy=', parentBusy)
        if (parentBusy) {
          // Parent's current turn will drain the queue at its next iteration —
          // no need to fire a sentinel turn (would just collide).
          console.log('[agent:idle] parent busy — skipping sentinel; current turn will drain')
        } else {
          import('@/lib/tauri')
            .then(({ sendMessage }) => {
              console.log('[agent:idle] firing sentinel sendMessage for', conversationId)
              return sendMessage(conversationId, '__resume_from_task_notification__')
            })
            .then(() => console.log('[agent:idle] sentinel sendMessage returned'))
            .catch((err) => console.warn('[agent:idle] resume sendMessage failed:', err))
        }
        return
      }

      console.log('[agent:idle] conversationId:', conversationId, 'scope:', effectiveScope, 'Agent finished, clearing busy state')
      recordDiagnostic({ event: 'agent.idle.received', conversationId, payload: { scope: effectiveScope, agentId } })
      flushConversationDeltas(conversationId)
      delete lastActivityRef.current[conversationId]
      const store = useChatStore.getState()
      store.removeBusyConversation(conversationId)
      // Safety net: also clear streaming state in case streaming:done was missed
      // (e.g. agent panicked before finish_agent could emit it)
      store.clearConversationStreamState(conversationId)
      useStreamingStore.getState().clearConversationPendingAsks(conversationId)
      useInteractionStore.getState().clearForConversation(conversationId)
    }),
  )

  // --- permission:ask ---------------------------------------------------
  useTauriEvent(() =>
    onPermissionAsk((payload: PermissionAskPayload) => {
      console.log('[permission:ask]', payload.conversationId, payload.toolName, payload.toolCallId)
      recordDiagnostic({
        event: 'permission.ask.received',
        conversationId: payload.conversationId,
        runId: payload.runId,
        toolCallId: payload.toolCallId,
        payload: { toolName: payload.toolName, mode: payload.mode },
      })
      useStreamingStore.getState().addPendingAsk(payload)
    }),
  )

  // --- interaction:required / interaction:resolved ------------------------
  useTauriEvent(() =>
    onInteractionRequired((payload: InteractionRequiredPayload) => {
      console.log('[interaction:required]', payload.conversationId, payload.toolName, payload.interactionId)
      recordDiagnostic({
        event: 'interaction.required.received',
        conversationId: payload.conversationId,
        runId: payload.runId,
        toolCallId: payload.toolCallId,
        interactionId: payload.interactionId,
        payload: { toolName: payload.toolName, kind: payload.kind },
      })
      useInteractionStore.getState().addInteraction(payload)
    }),
  )

  useTauriEvent(() =>
    onInteractionResolved((payload: InteractionResolvedPayload) => {
      useInteractionStore.getState().removeInteraction(payload.interactionId)
    }),
  )

  useTauriEvent(() =>
    onDiagnosticsEvent((payload: DiagnosticsEventPayload) => {
      useDiagnosticsStore.getState().appendDiagnostic(payload)
    }),
  )

  // --- task:status-changed ------------------------------------------------
  useTauriEvent(() =>
    onTaskStatusChanged((payload: TaskStatusChangedPayload) => {
      console.log('[task:status-changed]', payload.conversationId, payload.taskId, payload.status)
      useChatStore.getState().upsertConversationTaskState(payload.conversationId, {
        taskId: payload.taskId,
        status: payload.status,
        runId: payload.runId,
        subject: payload.subject,
        description: payload.description,
        activeForm: payload.activeForm,
        owner: payload.owner,
        blockedBy: payload.blockedBy,
        createdAt: payload.createdAt,
      })
    }),
  )

  // --- turn:completed ----------------------------------------------------
  useTauriEvent(() =>
    onTurnCompleted(({
      conversationId,
      outcome,
      totalInputTokens,
      totalOutputTokens,
      totalCostUsd,
    }: TurnCompletedPayload) => {
      console.log('[turn:completed]', conversationId, outcome, {
        totalInputTokens,
        totalOutputTokens,
        totalCostUsd,
      })

      // Non-success terminal outcomes should immediately release UI loading state
      // instead of waiting for the watchdog fallback.
      if (outcome !== 'Success') {
        flushConversationDeltas(conversationId)
        delete lastActivityRef.current[conversationId]
        const store = useChatStore.getState()
        store.clearConversationStreamState(conversationId)
        store.removeBusyConversation(conversationId)
      }

      useStreamingStore.getState().clearConversationPendingAsks(conversationId)

      switch (outcome) {
        case 'MaxIterationsReached':
          useNotificationStore.getState().push({
            level: 'warning',
            title: i18n.t('turnOutcome.maxIterationsTitle'),
            message: i18n.t('turnOutcome.maxIterationsDesc'),
            actions: [],
            dismissible: true,
            autoHide: 12,
            context: 'toast',
          })
          break
        case 'BudgetExceeded':
          useNotificationStore.getState().push({
            level: 'warning',
            title: i18n.t('turnOutcome.budgetExceededTitle'),
            message: i18n.t('turnOutcome.budgetExceededDesc'),
            actions: [],
            dismissible: true,
            autoHide: 12,
            context: 'toast',
          })
          break
        case 'ExecutionError':
          useNotificationStore.getState().push({
            level: 'error',
            title: i18n.t('turnOutcome.executionErrorTitle'),
            message: i18n.t('turnOutcome.executionErrorDesc'),
            actions: [],
            dismissible: true,
            autoHide: 10,
            context: 'toast',
          })
          break
        case 'Success':
          if (totalCostUsd != null && totalCostUsd > 0) {
            const tokens = (totalInputTokens ?? 0) + (totalOutputTokens ?? 0)
            useNotificationStore.getState().push({
              level: 'info',
              title: i18n.t('turnOutcome.successSummaryTitle'),
              message: i18n.t('turnOutcome.successSummaryDesc', {
                tokens,
                cost: totalCostUsd.toFixed(4),
              }),
              actions: [],
              dismissible: true,
              autoHide: 6,
              context: 'toast',
            })
          }
          break
        case 'Cancelled':
          break
      }

      useChatStore.getState().setLastTurnSummary(conversationId, {
        outcome,
        totalInputTokens,
        totalOutputTokens,
        totalCostUsd,
        completedAt: Date.now(),
      })
    }),
  )

  // --- turn:stage --------------------------------------------------------
  // Spec docs/superpowers/specs/2026-05-17-turn-stages.md §6.2.
  // Backend emits on every stage transition (Submitted / WaitingLlm / Tools /
  // WaitingPermission / WaitingInteraction / Completing).  Frontend stores the
  // raw stage; StreamingBubble derives a label from it.
  useTauriEvent(() =>
    onTurnStage(({ conversationId, stage, stageStartedAtMs }: TurnStagePayload) => {
      touchActivity(conversationId)
      recordDiagnostic({
        event: 'turn.stage.received',
        conversationId,
        payload: { kind: stage.kind, stageStartedAtMs },
      })
      useChatStore.getState().setConversationTurnStage(
        conversationId,
        stage,
        stageStartedAtMs,
      )
    }),
  )

  // --- turn:heartbeat ----------------------------------------------------
  // Spec §4.1.  ~2s keep-alive while a turn is active.  Refreshes the watchdog
  // timestamp so we don't false-positive a stuck turn during long silent
  // sub-stages (tool execution between events, model cold start, etc).
  useTauriEvent(() =>
    onTurnHeartbeat(({ conversationId }: TurnHeartbeatPayload) => {
      touchActivity(conversationId)
      useChatStore.getState().touchConversationHeartbeat(conversationId, Date.now())
    }),
  )

  // --- file:generated ----------------------------------------------------
  // Emitted directly by the tool execution layer (bypasses LLM).
  // Shows a warning toast when a file was degraded (e.g. PDF→HTML fallback).
  useTauriEvent(() =>
    onFileGenerated(({ conversationId, fileName, isDegraded, degradationNotice, requestedFormat, actualFormat }: FileGeneratedPayload) => {
      console.log('[file:generated]', conversationId, fileName, isDegraded ? 'DEGRADED' : 'ok')
      touchActivity(conversationId)
      if (isDegraded) {
        // Only show toast for the active conversation (avoid confusing cross-conversation toasts)
        const store = useChatStore.getState()
        if (conversationId !== store.activeConversationId) return
        // Deduplicate: skip if a similar degradation toast was shown within 5 seconds.
        // Notification IDs follow the pattern "notif_{counter}_{timestamp}".
        const notifStore = useNotificationStore.getState()
        const now = Date.now()
        const recentDuplicate = notifStore.notifications.some(
          (n) => n.context === 'toast' && n.title === 'File Format Changed'
            && now - Number(n.id.split('_').pop() ?? '0') < 5000,
        )
        if (!recentDuplicate) {
          notifStore.push({
            level: 'warning',
            title: 'File Format Changed',
            message: degradationNotice ?? `${requestedFormat.toUpperCase()} conversion failed, saved as ${actualFormat.toUpperCase()} instead.`,
            actions: [],
            dismissible: true,
            autoHide: 10,
            context: 'toast',
          })
        }
      }
    }),
  )

  // --- Stale stream watchdog -------------------------------------------
  // Periodically checks all conversations with isStreaming=true.
  // If no streaming activity (delta, tool event, step-reset) has been
  // received for STALE_STREAM_TIMEOUT_MS, force-clear the streaming
  // state. This prevents permanent UI freeze when Tauri events are lost.
  //
  // IMPORTANT: When a streaming session first starts (isStreaming=true set
  // in sendUserMessage), lastActivityRef has no entry yet because no
  // backend event has arrived. The watchdog must NOT clear this state
  // immediately — instead it initializes the activity timestamp on first
  // encounter so the full timeout applies from that point.
  useEffect(() => {
    const timer = setInterval(() => {
      const store = useChatStore.getState()
      const now = Date.now()

      for (const [convId, streamState] of Object.entries(store.streamStates)) {
        if (!streamState.isStreaming) continue

        const lastActive = lastActivityRef.current[convId]
        if (!lastActive) {
          // No activity ever recorded — this conversation just started streaming.
          // Initialize the timestamp so the full timeout applies from now.
          // Do NOT clear immediately; the first delta/tool event may still be
          // in transit (LLM cold start, checkpoint extraction, network latency).
          lastActivityRef.current[convId] = now
          continue
        }
        // A long-running tool (Bash/Write/Playwright/SubAgent) is silent on
        // the event bus between tool:executing and tool:completed. Per-tool
        // timeouts (default 120s, max 600s) guarantee tool:completed eventually
        // fires, so reset the watchdog while any tool is in-flight to avoid
        // false-positive "stream timeout" toasts during legitimate work.
        const hasExecutingTool = streamState.toolExecutions.some(
          (t) => t.status === 'executing',
        )
        if (hasExecutingTool) {
          lastActivityRef.current[convId] = now
          continue
        }
        if (now - lastActive > STALE_STREAM_TIMEOUT_MS) {
          console.warn(
            '[watchdog] Force-clearing stale streaming state for conversation %s (last activity: %s ms ago)',
            convId,
            now - lastActive,
          )
          recordDiagnostic({
            event: 'streaming.watchdog.stale_detected',
            conversationId: convId,
            payload: { lastActivityAt: lastActive, now },
          })
          delete deltaBufferRef.current[convId]
          delete lastActivityRef.current[convId]
          store.clearConversationStreamState(convId)
          store.removeBusyConversation(convId)

          // Show user-friendly notification
          useNotificationStore.getState().push({
            level: 'warning',
            title: i18n.t('errors.streamTimeout'),
            message: i18n.t('errors.streamTimeoutDesc', { seconds: STALE_STREAM_TIMEOUT_MS / 1000 }),
            actions: [],
            dismissible: true,
            autoHide: 10,
            context: 'toast',
          })
        }
      }
    }, WATCHDOG_INTERVAL_MS)

    return () => clearInterval(timer)
  }, [])
}
