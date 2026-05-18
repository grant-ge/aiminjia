/**
 * StreamingBubble — shows the AI response as it streams in, plus a derived
 * "what is the agent doing right now" label.
 *
 * Spec: docs/superpowers/specs/2026-05-17-turn-stages.md §6.3.
 *
 * The visible status text is derived from `turnStage` (single source of truth
 * pushed by the backend).  When `turnStage` is null (feature flag off, or the
 * first `turn:stage` event hasn't landed yet) we fall back to the legacy
 * behaviour (tool name from `toolExecutions[]` or generic "思考中…").
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { TFunction } from 'i18next'

import { useChatStore } from '@/stores/chatStore'
import type { TurnStageKind } from '@/lib/tauri'
import { TypingIndicator } from '@/components/chat-scene/TypingIndicator'
import { AssistantMarkdown } from '@/components/chat-scene/AssistantMarkdown'
import { stripHallucinatedXml } from '@/lib/sanitize'

interface StreamingBubbleProps {
  content: string
}

interface StatusDescriptor {
  icon: 'spin' | 'breath'
  label: string
}

/** When stage hasn't arrived yet, fall back to the existing tool-name /
 *  generic "思考中…" derivation so PR1 with the flag off behaves identically
 *  to pre-PR1. */
function legacyDerive(
  toolExecutions: Array<{ toolName: string; status: string }>,
  hasContent: boolean,
  t: TFunction,
): StatusDescriptor {
  const activeTool = toolExecutions.find((tool) => tool.status === 'executing')
  if (activeTool) {
    return {
      icon: 'spin',
      label: t('streaming.tools.' + activeTool.toolName, activeTool.toolName),
    }
  }
  return {
    icon: 'breath',
    label: hasContent ? '' : t('turnStage.fallback'),
  }
}

function deriveStageStatus(
  stage: TurnStageKind | null | undefined,
  stageStartedAt: number | null | undefined,
  lastHeartbeatAt: number | null | undefined,
  hasContent: boolean,
  now: number,
  t: TFunction,
  toolExecutions: Array<{ toolName: string; status: string }>,
): StatusDescriptor {
  if (!stage) {
    return legacyDerive(toolExecutions, hasContent, t)
  }

  // 30s without a heartbeat → degrade label to "stalled" (warning tone).
  const stalled =
    typeof lastHeartbeatAt === 'number' && now - lastHeartbeatAt > 30_000

  const elapsedSec =
    typeof stageStartedAt === 'number'
      ? Math.max(0, Math.floor((now - stageStartedAt) / 1000))
      : 0
  const elapsedSuffix = elapsedSec >= 3 ? t('turnStage.elapsed', { sec: elapsedSec }) : ''

  if (stalled) {
    return { icon: 'breath', label: t('turnStage.stalled') }
  }

  switch (stage.kind) {
    case 'submitted':
      return { icon: 'breath', label: t('turnStage.submitted') }
    case 'waitingLlm':
      // Once any content has streamed, the markdown bubble itself is the
      // "we're alive" signal — hide the label so we don't double-render
      // "等待模型响应 · 已 12s" alongside fresh text.  Same UX as Streaming.
      return {
        icon: 'breath',
        label: hasContent ? '' : t('turnStage.waitingLlm') + elapsedSuffix,
      }
    case 'streaming':
      return {
        icon: 'breath',
        label: hasContent ? '' : t('turnStage.streaming'),
      }
    case 'tools': {
      // Prefer the live executing-tool count from toolExecutions[] over the
      // batch-snapshot in stage.running.  As individual tools complete, the
      // live count shrinks naturally without needing per-tool stage updates.
      const liveRunning = toolExecutions.filter((tool) => tool.status === 'executing')
      const n = liveRunning.length > 0 ? liveRunning.length : stage.running.length
      const firstName = liveRunning[0]?.toolName ?? stage.running[0]?.toolName
      if (n === 0) {
        return { icon: 'breath', label: t('turnStage.toolsPreparing') }
      }
      if (n === 1 && firstName) {
        return {
          icon: 'spin',
          label: t('turnStage.toolSingle', { name: firstName }) + elapsedSuffix,
        }
      }
      return {
        icon: 'spin',
        label: t('turnStage.toolsMulti', { count: n }) + elapsedSuffix,
      }
    }
    case 'waitingPermission':
      return {
        icon: 'breath',
        label: t('turnStage.waitingPermission', { name: stage.toolName }),
      }
    case 'waitingInteraction':
      return { icon: 'breath', label: t('turnStage.waitingInteraction') }
    case 'compacting':
      return { icon: 'spin', label: t('turnStage.compacting') }
    case 'completing':
      return { icon: 'breath', label: t('turnStage.completing') }
  }
}

/** Re-render once per second so the "已 12s" suffix actually advances.
 *  Cheap: only the bubble subscribes. */
function useTick(intervalMs: number): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), intervalMs)
    return () => clearInterval(timer)
  }, [intervalMs])
  return now
}

export function StreamingBubble({ content }: StreamingBubbleProps) {
  const { t } = useTranslation()
  const toolExecutions = useChatStore((s) => {
    const activeId = s.activeConversationId
    return activeId ? (s.streamStates[activeId]?.toolExecutions ?? []) : []
  })
  const turnStage = useChatStore((s) => {
    const activeId = s.activeConversationId
    return activeId ? (s.streamStates[activeId]?.turnStage ?? null) : null
  })
  const stageStartedAt = useChatStore((s) => {
    const activeId = s.activeConversationId
    return activeId ? (s.streamStates[activeId]?.stageStartedAt ?? null) : null
  })
  const lastHeartbeatAt = useChatStore((s) => {
    const activeId = s.activeConversationId
    return activeId ? (s.streamStates[activeId]?.lastHeartbeatAt ?? null) : null
  })

  const cleanContent = stripHallucinatedXml(content)
  const hasContent = cleanContent.length > 0
  const now = useTick(1_000)

  const status = deriveStageStatus(
    turnStage,
    stageStartedAt,
    lastHeartbeatAt,
    hasContent,
    now,
    t,
    toolExecutions,
  )

  return (
    <div className="mb-7">
      <div>
        {hasContent ? <AssistantMarkdown text={cleanContent} /> : null}
        {status.icon === 'spin' ? (
          <div
            className="mt-2 flex items-center gap-2 text-xs"
            style={{ color: 'var(--color-text-muted)' }}
          >
            <svg
              className="h-3.5 w-3.5 animate-spin"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2.5"
            >
              <circle
                cx="12"
                cy="12"
                r="10"
                strokeDasharray="50"
                strokeDashoffset="20"
                strokeLinecap="round"
              />
            </svg>
            <span>{status.label}</span>
          </div>
        ) : (
          <div className={hasContent ? 'mt-2' : ''}>
            <TypingIndicator variant="default" label={status.label || undefined} />
            {status.label && hasContent ? (
              <span className="sr-only">{status.label}</span>
            ) : null}
          </div>
        )}
      </div>
    </div>
  )
}
