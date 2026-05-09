import { useCallback, useEffect, useRef, useState } from 'react'

import { type AgendaItem, type ItemFilter, listAgendaItems } from '@/lib/tauri'

export function useAgendaItems(filter?: ItemFilter) {
  const [items, setItems] = useState<AgendaItem[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const mountedRef = useRef(true)
  const requestSeqRef = useRef(0)

  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
    }
  }, [])

  const load = useCallback(async (shouldCommit: () => boolean = () => true) => {
    if (!mountedRef.current) return
    const requestSeq = requestSeqRef.current + 1
    requestSeqRef.current = requestSeq
    setLoading(true)
    setError(null)
    try {
      const next = await listAgendaItems(filter)
      if (mountedRef.current && shouldCommit() && requestSeq === requestSeqRef.current) {
        setItems(next)
      }
    } catch (e) {
      if (mountedRef.current && shouldCommit() && requestSeq === requestSeqRef.current) {
        setError(e instanceof Error ? e.message : String(e))
      }
    } finally {
      if (mountedRef.current && shouldCommit() && requestSeq === requestSeqRef.current) {
        setLoading(false)
      }
    }
  }, [filter])

  const refresh = useCallback(async () => {
    await load()
  }, [load])

  useEffect(() => {
    let cancelled = false
    void load(() => !cancelled)
    return () => {
      cancelled = true
    }
  }, [load])

  return { items, loading, error, refresh }
}
