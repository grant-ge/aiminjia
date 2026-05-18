/**
 * InterruptedTurnBanner — spec 2026-05-17-turn-stages §5.5.
 *
 * Renders a one-line banner at the top of a chat conversation when the
 * previous backend process died mid-turn for this conversation.  Reads the
 * sentinel via `getInterruptedTurn`; user can dismiss or resend the last
 * user message.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import {
  dismissInterruptedTurn,
  getInterruptedTurn,
  type InterruptedTurnRecord,
  type TurnStageKind,
} from '@/lib/tauri'
import { useChat } from '@/hooks/useChat'
import { useChatStore } from '@/stores/chatStore'

interface InterruptedTurnBannerProps {
  conversationId: string
}

function stageLabel(stage: TurnStageKind, t: (key: string, opts?: object) => string): string {
  switch (stage.kind) {
    case 'submitted':           return t('turnStage.submitted')
    case 'waitingLlm':          return t('turnStage.waitingLlm')
    case 'streaming':           return t('turnStage.streaming')
    case 'tools':
      return stage.running[0]?.toolName
        ? t('turnStage.toolSingle', { name: stage.running[0].toolName })
        : t('turnStage.toolsPreparing')
    case 'waitingPermission':   return t('turnStage.waitingPermission', { name: stage.toolName })
    case 'waitingInteraction':  return t('turnStage.waitingInteraction')
    case 'compacting':          return t('turnStage.compacting')
    case 'completing':          return t('turnStage.completing')
  }
}

export function InterruptedTurnBanner({ conversationId }: InterruptedTurnBannerProps) {
  const { t } = useTranslation()
  // Pair the record with the conversationId it was fetched for, so a stale
  // record from a prior conversation can't leak across a switch.  Derive the
  // visible record from prop equality instead of resetting in useEffect
  // (avoids the set-state-in-effect lint).
  const [scoped, setScoped] = useState<{
    convId: string
    record: InterruptedTurnRecord
  } | null>(null)
  const { sendUserMessage } = useChat()

  useEffect(() => {
    let cancelled = false
    void getInterruptedTurn(conversationId)
      .then((found) => {
        if (cancelled) return
        if (found) setScoped({ convId: conversationId, record: found })
      })
      .catch((err) => {
        console.warn('[InterruptedTurnBanner] getInterruptedTurn failed:', err)
      })
    return () => {
      cancelled = true
    }
  }, [conversationId])

  const record = scoped?.convId === conversationId ? scoped.record : null
  if (!record) return null

  const handleDismiss = async () => {
    setScoped(null)
    try {
      await dismissInterruptedTurn(conversationId)
    } catch (err) {
      console.warn('[InterruptedTurnBanner] dismiss failed:', err)
    }
  }

  const handleResend = async () => {
    // Find the last user message in the current conversation and resend its text.
    const messages = useChatStore.getState().messages
    const lastUser = [...messages]
      .reverse()
      .find((m) => m.role === 'user' && m.conversationId === conversationId)
    setScoped(null)
    try {
      await dismissInterruptedTurn(conversationId)
    } catch (err) {
      console.warn('[InterruptedTurnBanner] dismiss-before-resend failed:', err)
    }
    if (lastUser?.content.text) {
      await sendUserMessage(lastUser.content.text)
    }
  }

  return (
    <div className="mx-2 my-2 flex items-center justify-between gap-3 rounded-md border border-border bg-muted px-3 py-2 text-sm text-muted-foreground">
      <span>
        {t('turnStage.interruptedBanner', { stage: stageLabel(record.lastStage, t) })}
      </span>
      <div className="flex items-center gap-2">
        <Button size="sm" variant="default" onClick={() => void handleResend()}>
          {t('turnStage.interruptedRetry')}
        </Button>
        <Button size="sm" variant="ghost" onClick={() => void handleDismiss()}>
          {t('turnStage.interruptedDismiss')}
        </Button>
      </div>
    </div>
  )
}
