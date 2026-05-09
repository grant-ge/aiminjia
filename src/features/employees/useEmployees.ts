import { useCallback, useEffect, useState } from 'react'
import {
  employeeActiveRun,
  employeeList,
  type EmployeeActiveRunInfo,
  type EmployeeRecord,
} from '@/lib/tauri'

const POLL_MS = 5_000

export function useEmployees() {
  const [employees, setEmployees] = useState<EmployeeRecord[]>([])
  const [activeRuns, setActiveRuns] = useState<Record<string, EmployeeActiveRunInfo>>({})
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setError(null)
    const list = await employeeList()
    setEmployees(list)
    // Probe active run only for non-archived employees. Archived employees
    // can't have a real run by definition (the trigger command rejects them
    // and the scheduler skips them), and on a recycle bin with N entries we
    // were burning N IPC calls every 5s for nothing.
    const probes = await Promise.all(
      list
        .filter((e) => e.lifecycle !== 'archived')
        .map(async (e) => {
          try {
            return [e.id, await employeeActiveRun(e.id)] as const
          } catch (err) {
            console.warn(`[useEmployees] employeeActiveRun(${e.id}) failed:`, err)
            return [e.id, null] as const
          }
        }),
    )
    const next: Record<string, EmployeeActiveRunInfo> = {}
    for (const [id, run] of probes) {
      if (run) next[id] = run
    }
    setActiveRuns(next)
  }, [])

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    refresh()
      .catch((err) => {
        if (!cancelled) {
          setError(String(err))
          console.error('[useEmployees] initial refresh failed:', err)
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    const t = setInterval(() => {
      if (cancelled) return
      refresh().catch((err) => console.error('[useEmployees] poll failed:', err))
    }, POLL_MS)
    return () => {
      cancelled = true
      clearInterval(t)
    }
  }, [refresh])

  return { employees, activeRuns, loading, error, refresh }
}
