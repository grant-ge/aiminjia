import { useCallback, useEffect, useRef, useState } from 'react'
import { inboxList, inboxMarkAllRead, inboxMarkRead, inboxUnreadCount, type InboxEntry } from '@/lib/tauri'

const POLL_INTERVAL_MS = 30_000

export function useInbox(employeeId?: string) {
  const [entries, setEntries] = useState<InboxEntry[]>([])
  const [unreadCount, setUnreadCount] = useState(0)
  const [loading, setLoading] = useState(true)

  const refresh = useCallback(async () => {
    const [items, count] = await Promise.all([
      inboxList(employeeId, 100),
      inboxUnreadCount(employeeId),
    ])
    setEntries(items)
    setUnreadCount(count)
  }, [employeeId])

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    refresh()
      .catch(() => {})
      .finally(() => { if (!cancelled) setLoading(false) })

    const timer = setInterval(() => {
      if (!cancelled) refresh().catch(() => {})
    }, POLL_INTERVAL_MS)

    return () => {
      cancelled = true
      clearInterval(timer)
    }
  }, [refresh])

  const markRead = useCallback(async (empId: string, entryId: string) => {
    await inboxMarkRead(empId, entryId)
    await refresh()
  }, [refresh])

  const markAllRead = useCallback(async (empId: string) => {
    await inboxMarkAllRead(empId)
    await refresh()
  }, [refresh])

  return { entries, unreadCount, loading, refresh, markRead, markAllRead }
}

/** Global unread count badge — polled every 30s. */
export function useGlobalUnreadCount() {
  const [count, setCount] = useState(0)
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const refresh = useCallback(async () => {
    const n = await inboxUnreadCount()
    setCount(n)
  }, [])

  useEffect(() => {
    refresh().catch(() => {})
    timerRef.current = setInterval(() => { refresh().catch(() => {}) }, POLL_INTERVAL_MS)
    return () => {
      if (timerRef.current) clearInterval(timerRef.current)
    }
  }, [refresh])

  return count
}
