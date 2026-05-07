import { useCallback, useEffect, useState } from 'react'

import { AgendaItem, ItemFilter, listAgendaItems } from '@/lib/tauri'

export function useAgendaItems(filter?: ItemFilter) {
  const [items, setItems] = useState<AgendaItem[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const next = await listAgendaItems(filter)
      setItems(next)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [filter])

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const next = await listAgendaItems(filter)
        if (!cancelled) setItems(next)
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e))
      }
    })()
    return () => {
      cancelled = true
    }
  }, [filter])

  return { items, loading, error, refresh }
}
