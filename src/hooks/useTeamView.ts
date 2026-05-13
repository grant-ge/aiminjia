import { useCallback, useEffect, useRef, useState } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { listenTeamMessage, TAURI_EVENTS, teamViewForConversation } from '@/lib/tauri'
import type { TeamEvent, TeamMessage, TeamView } from '@/types/team'

interface UseTeamViewState {
  view: TeamView | null
  loading: boolean
  error: string | null
  refresh: () => void
}

const EMPTY_VIEW: TeamView = {
  events: [],
  roster: {
    team_name: null,
    description: null,
    created_at: null,
    members: [],
    task_count_total: 0,
    task_count_completed: 0,
  },
}

function messageToEvent(msg: TeamMessage): TeamEvent {
  return {
    kind: 'message_sent',
    ts: msg.ts,
    sender: msg.from,
    to: msg.to,
    content: msg.body,
    anchor_message_id: null,
  }
}

/**
 * 拉取当前对话的群聊视图，并在收到 turn:completed / tool:completed /
 * message:updated / streaming:done 等"对话流变化"事件时自动重拉历史；
 * 实时通过 team:message 增量追加（避免每条消息都重扫 transcript）。
 *
 * 历史 events 由后端 team_view_for_conversation 通过反扫 transcript 派生；
 * 没有 conversationId 时不发请求，view=null。
 */
export function useTeamView(conversationId: string | null): UseTeamViewState {
  const [view, setView] = useState<TeamView | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const inflightRef = useRef<symbol | null>(null)

  const refresh = useCallback(() => {
    if (!conversationId) {
      setView(null)
      setError(null)
      return
    }
    const token = Symbol('teamview-fetch')
    inflightRef.current = token
    setLoading(true)
    teamViewForConversation(conversationId)
      .then((next) => {
        if (inflightRef.current !== token) return
        setView(next ?? EMPTY_VIEW)
        setError(null)
      })
      .catch((e: unknown) => {
        if (inflightRef.current !== token) return
        const msg = e instanceof Error ? e.message : String(e)
        setError(msg)
      })
      .finally(() => {
        if (inflightRef.current === token) setLoading(false)
      })
  }, [conversationId])

  useEffect(() => {
    refresh()
  }, [refresh])

  useEffect(() => {
    if (!conversationId) return
    let unlisteners: UnlistenFn[] = []
    let alive = true

    const TRIGGERS = [
      TAURI_EVENTS.TURN_COMPLETED,
      TAURI_EVENTS.TOOL_COMPLETED,
      TAURI_EVENTS.MESSAGE_UPDATED,
      TAURI_EVENTS.STREAMING_DONE,
    ]

    Promise.all(
      TRIGGERS.map((ev) =>
        listen(ev, (payload) => {
          const conv =
            (payload?.payload as { conversationId?: string; conversation_id?: string } | null)
              ?.conversationId ??
            (payload?.payload as { conversationId?: string; conversation_id?: string } | null)
              ?.conversation_id ??
            null
          if (conv && conv !== conversationId) return
          refresh()
        }),
      ),
    ).then((fns) => {
      if (!alive) {
        fns.forEach((u) => u())
        return
      }
      unlisteners = fns
    })

    return () => {
      alive = false
      unlisteners.forEach((u) => u())
    }
  }, [conversationId, refresh])

  // team:message 增量：直接 append 到 events，避免重扫 transcript。
  // 下次 refresh（turn 完成等）会用后端反扫的版本覆盖整个 events 列表，
  // 把过去的乐观追加重新归位——所以即便这里偶有重复也会被刷新自然纠正。
  useEffect(() => {
    if (!conversationId) return
    let alive = true
    let unlisten: UnlistenFn | null = null
    listenTeamMessage((msg) => {
      if (!alive) return
      if (msg.conversationId !== conversationId) return
      setView((prev) => {
        if (!prev) return prev
        return {
          ...prev,
          events: [...prev.events, messageToEvent(msg)],
        }
      })
    }).then((u) => {
      if (!alive) {
        u()
        return
      }
      unlisten = u
    })

    return () => {
      alive = false
      if (unlisten) unlisten()
    }
  }, [conversationId])

  return { view, loading, error, refresh }
}
