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
  /** 关掉 typing/spin indicator，只渲染 markdown。inline 路径用这个 prop
   *  避免和末尾独立挂载的 indicator-only placeholder 重复。 */
  suppressIndicator?: boolean
  /** 外部告诉 StreamingBubble "这个 turn 已经有内容了"，让 derive 走
   *  hasContent=true 分支（不显示"等待模型响应/正在生成"这类初始 label）。
   *  末尾 placeholder StreamingBubble 自身 content="" 但所处 turn 已经有
   *  persisted blocks，必须传 true，否则会误显示初始 label。 */
  treatAsHasContent?: boolean
}

interface StatusDescriptor {
  icon: 'spin' | 'breath'
  label: string
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
    // Stage 未到达：流式刚开始 / 旧会话。任何正在执行的工具都不在这里渲染
    // spinner——下方 ToolStepGroupBlock 已经独占运行态展示。
    return { icon: 'breath', label: hasContent ? '' : t('turnStage.fallback') }
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
      // 交错模式（唯一渲染模式）下，下方 ToolStepGroupBlock 已显示 spinner +
      // 工具名/计数，这里只展示"思考中…" typing 占位避免视觉重复。
      const liveRunning = toolExecutions.filter((tool) => tool.status === 'executing')
      if (liveRunning.length === 0 && stage.running.length === 0) {
        return { icon: 'breath', label: t('turnStage.toolsPreparing') }
      }
      return { icon: 'breath', label: hasContent ? '' : t('turnStage.fallback') }
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

// Stable empty array reference so Zustand selectors that fall back to []
// don't return a new array every render (which would cause useSyncExternalStore
// to detect a "changed" snapshot every tick and recurse into Maximum-update-depth).
const EMPTY_TOOL_EXECUTIONS: never[] = []

export function StreamingBubble({
  content,
  suppressIndicator = false,
  treatAsHasContent = false,
}: StreamingBubbleProps) {
  const { t } = useTranslation()
  const toolExecutions = useChatStore((s) => {
    const activeId = s.activeConversationId
    return activeId ? (s.streamStates[activeId]?.toolExecutions ?? EMPTY_TOOL_EXECUTIONS) : EMPTY_TOOL_EXECUTIONS
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
  const hasContent = cleanContent.length > 0 || treatAsHasContent
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

  // typing/spin loading 块用 absolute 脱离文档流：
  // - hasContent=true：top-full + mt-2 锚到 markdown 末尾下方 8px（跟原 inline
  //   时同位）；layout 高度不再算 indicator，流式 delta 只按真实文字增量长高，
  //   避免 stick-to-bottom 来回追 indicator 高度产生的"撑开 + 闪"。
  // - hasContent=false：top-0 顶到容器起点（跟原 mt-0 同位）。indicator-only
  //   placeholder（末尾追加的 content="" StreamingBubble）走这条。
  // 父层 mb-7 给 indicator 预留视觉空间，不会跟下一个 sibling 紧贴。
  const indicatorPositionClass = hasContent ? 'top-full mt-2' : 'top-0'
  return (
    <div className="mb-7" data-aijia-streaming-bubble>
      <div className="relative">
        {hasContent ? <AssistantMarkdown text={cleanContent} disableCodeHighlight /> : null}
        {suppressIndicator ? null : status.icon === 'spin' ? (
          <div
            className={`absolute left-0 ${indicatorPositionClass} flex items-center gap-2 text-xs`}
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
          <div className={`absolute left-0 ${indicatorPositionClass}`}>
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
