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
import { Spinner } from '@/components/ui/spinner'

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
  // 两个独立信号：
  // - hasMarkdown：实际有可见文本要画。决定是否渲染 <AssistantMarkdown> 和
  //   indicator 的定位分支（top-full mt-2 锚 markdown 末尾 / top-0 贴容器顶）。
  // - hasContent：传给 deriveStageStatus 压初始 label（"等待模型响应"等）。
  //   treatAsHasContent=true 的占位场景仍然算作"已有内容"——上方有 persisted
  //   blocks 在撑场子，不该再喊"等待模型响应"。
  const hasMarkdown = cleanContent.length > 0
  const hasContent = hasMarkdown || treatAsHasContent
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

  // 全空快路径：既没 markdown 又把 indicator 也 suppress 掉，wrapper 是个 0
  // 内容的 <div class="mb-7"><div class="relative"></div></div>，但仍然在父
  // ChatRow 里占一个 flex item，触发 gap-3 + mb-7 撑出空气块。流式 inline
  // bubble 在 stripHallucinatedXml 把整段 <function_calls>… 砍光、且本身就是
  // suppressIndicator 模式时会走到这里——直接 return null 让 layout 闭合。
  if (!hasMarkdown && suppressIndicator) return null

  // typing/spin loading 块用 absolute 脱离文档流：
  // - hasMarkdown=true：top-full + mt-2 锚到 markdown 末尾下方 8px（跟原 inline
  //   时同位）；layout 高度不再算 indicator，流式 delta 只按真实文字增量长高，
  //   避免 stick-to-bottom 来回追 indicator 高度产生的"撑开 + 闪"。
  // - hasMarkdown=false：top-0 顶到容器起点（跟原 mt-0 同位）。indicator-only
  //   placeholder（末尾追加的 content="" StreamingBubble）走这条——即便
  //   treatAsHasContent=true 也走 top-0，因为容器内没有真实 markdown 内容。
  const indicatorPositionClass = hasMarkdown ? 'top-full mt-2' : 'top-0'
  // mb-7 只在 case 3（markdown + 渲 indicator）需要：indicator 是 absolute
  // 脱离文档流，必须靠父 mb-7 给它在文档流里挖 28px 占位防撞下一 sibling。
  // - case 1 inline + suppressIndicator: 无 indicator → mb-0，父级 ChatRow gap-1
  //   接管间距，跟 AiBubble 落盘后对齐，stream→persisted 不抖动
  // - case 2 末尾 placeholder (content="" + dots): dots 下方由父级 turn gap-5
  //   兜底 → mb-0，避免 dots 下方 28px 拖尾
  const needsBottomReserve = hasMarkdown && !suppressIndicator
  return (
    <div
      className={needsBottomReserve ? 'mb-7' : ''}
      data-aijia-streaming-bubble
    >
      <div className="relative">
        {hasMarkdown ? <AssistantMarkdown text={cleanContent} disableCodeHighlight /> : null}
        {suppressIndicator ? null : status.icon === 'spin' ? (
          <div
            className={`absolute left-0 ${indicatorPositionClass} flex items-center gap-2 text-xs`}
            style={{ color: 'var(--color-text-muted)' }}
          >
            <Spinner size="sm" />
            <span>{status.label}</span>
          </div>
        ) : (
          <div className={`absolute left-0 ${indicatorPositionClass}`}>
            <TypingIndicator variant="default" label={status.label || undefined} />
            {status.label && hasMarkdown ? (
              <span className="sr-only">{status.label}</span>
            ) : null}
          </div>
        )}
      </div>
    </div>
  )
}
