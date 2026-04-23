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
import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import i18n from '@/i18n'
import type { Message } from '@/types/message'
import {
  onStreamingDelta,
  onStreamingDone,
  onStreamingError,
  onMessageUpdated,
  onToolExecuting,
  onToolCompleted,
  onAnalysisStepChanged,
  onAgentIdle,
  onAgentPhase,
  onPermissionAsk,
  onStreamingStepReset,
  onFileGenerated,
  onTaskStatusChanged,
  onTurnCompleted,
} from '@/lib/tauri'
import type {
  StreamingDeltaPayload,
  StreamingDonePayload,
  StreamingErrorPayload,
  AgentIdlePayload,
  AgentPhasePayload,
  ToolExecutingPayload,
  PermissionAskPayload,
  StreamingStepResetPayload,
  FileGeneratedPayload,
  TaskStatusChangedPayload,
  TurnCompletedPayload,
} from '@/lib/tauri'
import { useAnalysisStore } from '@/stores/analysisStore'
import type { StepStatus } from '@/types/analysis'
import { useStreamingStore } from '@/stores/streamingStore'
import { useTauriEvent } from './useTauriEvent'

/** How long (ms) before a streaming conversation with no activity is force-cleared.
 *  Set to 200s to accommodate analysis mode step transitions which involve
 *  checkpoint extraction (up to 30s) + LLM cold start + first tool execution.
 *  Must exceed backend's analysis chunk timeout (180s in chat.rs). */
const STALE_STREAM_TIMEOUT_MS = 200_000

/** How often (ms) the watchdog checks for stale streams. */
const WATCHDOG_INTERVAL_MS = 10_000

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
      useChatStore.getState().appendConversationStreamingContent(conversationId, buffered)
    }
    delete deltaBufferRef.current[conversationId]
  }

  // --- streaming:delta -------------------------------------------------
  useTauriEvent(() =>
    onStreamingDelta(({ conversationId, delta }: StreamingDeltaPayload) => {
      touchActivity(conversationId)
      // Buffer the delta instead of immediately updating the store
      deltaBufferRef.current[conversationId] =
        (deltaBufferRef.current[conversationId] ?? '') + delta
      scheduleFlush()
    }),
  )

  // --- streaming:done --------------------------------------------------
  useTauriEvent(() =>
    onStreamingDone(({ conversationId }: StreamingDonePayload) => {
      console.log('[streaming:done] conversationId:', conversationId)
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
    onStreamingError(({ conversationId, error, errorType, partialContent }: StreamingErrorPayload) => {
      console.error('[streaming:error]', conversationId, errorType ?? 'unknown', error)
      // Flush buffered deltas so partial content is preserved before clearing
      flushConversationDeltas(conversationId)
      delete lastActivityRef.current[conversationId]
      const store = useChatStore.getState()
      store.clearConversationStreamState(conversationId)
      store.removeBusyConversation(conversationId)

      // Show longer auto-hide for timeout errors (user needs time to read)
      const autoHideSecs = errorType === 'chunk_timeout' || errorType === 'agent_timeout' ? 15 : 8

      const suffix = partialContent ? i18n.t('errors.partialSaved') : ''

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
      console.log('[message:updated] id:', message.id, 'role:', message.role, 'convId:', message.conversationId)
      const store = useChatStore.getState()
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
      // unmounts (streaming:done) before the persisted MessageItem appears.
      if (message.role === 'assistant') {
        const streamState = store.streamStates[message.conversationId]
        if (streamState?.isStreaming) {
          console.log('[message:updated] Clearing streaming state for %s (assistant message persisted)', message.conversationId)
          flushConversationDeltas(message.conversationId)
          delete lastActivityRef.current[message.conversationId]
          store.clearConversationStreamState(message.conversationId)
        }
      }
    }),
  )

  // --- tool:executing ---------------------------------------------------
  useTauriEvent(() =>
    onToolExecuting(({ conversationId, toolName, toolId, purpose, input }: ToolExecutingPayload) => {
      console.log('[tool:executing]', conversationId, toolName, toolId)
      touchActivity(conversationId)
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
      console.log('[tool:completed]', message.conversationId, message.toolResult?.name)
      touchActivity(message.conversationId)
      useChatStore.getState().upsertMessage(message)
      if (message.toolResult) {
        useChatStore.getState().updateConversationToolExecution(
          message.conversationId,
          message.toolResult.toolCallId,
          {
            status: message.toolResult.isError ? 'error' : 'completed',
            durationMs: message.toolResult.durationMs,
          },
        )
      }
    }),
  )

  // --- analysis:step-changed --------------------------------------------
  useTauriEvent(() =>
    onAnalysisStepChanged(({ step, status }) => {
      console.log('[analysis:step-changed]', step, status)
      const store = useAnalysisStore.getState()
      store.setCurrentStep(step)
      store.setStepStatus(step, status as StepStatus)
    }),
  )

  // --- streaming:step-reset -----------------------------------------------
  // Emitted when the backend auto-advances to a new analysis step.
  // Clears the previous step's streaming content and tool executions,
  // but keeps isStreaming=true so StreamingBubble stays visible.
  useTauriEvent(() =>
    onStreamingStepReset(({ conversationId, step }: StreamingStepResetPayload) => {
      console.log('[streaming:step-reset] conversationId:', conversationId, 'step:', step)
      touchActivity(conversationId)
      // Discard buffered deltas from the previous step
      delete deltaBufferRef.current[conversationId]
      useChatStore.getState().resetConversationStreamContent(conversationId)
    }),
  )

  // --- agent:phase --------------------------------------------------------
  useTauriEvent(() =>
    onAgentPhase(({ conversationId, phase }: AgentPhasePayload) => {
      useChatStore.getState().setConversationAgentPhase(conversationId, phase)
    }),
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
        return
      }

      console.log('[agent:idle] conversationId:', conversationId, 'scope:', effectiveScope, 'Agent finished, clearing busy state')
      flushConversationDeltas(conversationId)
      delete lastActivityRef.current[conversationId]
      const store = useChatStore.getState()
      store.removeBusyConversation(conversationId)
      // Safety net: also clear streaming state in case streaming:done was missed
      // (e.g. agent panicked before finish_agent could emit it)
      store.clearConversationStreamState(conversationId)
      useStreamingStore.getState().clearConversationPendingAsks(conversationId)
    }),
  )

  // --- permission:ask ---------------------------------------------------
  useTauriEvent(() =>
    onPermissionAsk((payload: PermissionAskPayload) => {
      console.log('[permission:ask]', payload.conversationId, payload.toolName, payload.toolCallId)
      useStreamingStore.getState().addPendingAsk(payload)
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
        if (now - lastActive > STALE_STREAM_TIMEOUT_MS) {
          console.warn(
            '[watchdog] Force-clearing stale streaming state for conversation %s (last activity: %s ms ago)',
            convId,
            now - lastActive,
          )
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
