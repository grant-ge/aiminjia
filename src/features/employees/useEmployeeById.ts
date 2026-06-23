import { useEffect, useState } from 'react'
import { employeeList, type EmployeeRecord } from '@/lib/tauri'

/**
 * Lightweight single-employee lookup, used by `ChatTopBar` to render the
 * employee identity card on dispatch conversations.
 *
 * Why not reuse `useEmployees`? That hook polls `employeeActiveRun` every
 * 5s for every employee — way too noisy for a top-bar component that only
 * needs name/role/avatar of one employee on mount.
 *
 * Caches by id in module-level Map so switching between dispatch
 * conversations doesn't re-fetch the full list every time.
 */
const cache = new Map<string, EmployeeRecord | null>()
let inflight: Promise<void> | null = null

async function ensureList(id?: string) {
  if (id ? cache.has(id) : cache.size > 0) return
  if (inflight) return inflight
  inflight = (async () => {
    try {
      const list = await employeeList()
      for (const e of list) cache.set(e.id, e)
    } catch (err) {
      console.warn('[useEmployeeById] employeeList failed:', err)
    } finally {
      inflight = null
    }
  })()
  return inflight
}

export function useEmployeeById(id: string | null | undefined): EmployeeRecord | null {
  const [emp, setEmp] = useState<EmployeeRecord | null>(() =>
    id ? (cache.get(id) ?? null) : null,
  )

  useEffect(() => {
    let cancelled = false
    ;(async () => {
      if (!id) {
        await Promise.resolve()
        if (!cancelled) setEmp(null)
        return
      }
      await ensureList(id)
      if (!cancelled) setEmp(cache.get(id) ?? null)
    })()
    return () => {
      cancelled = true
    }
  }, [id])

  return emp
}

/** Test-only helper. Not exported in the production bundle. */
export function __resetEmployeeByIdCacheForTesting() {
  cache.clear()
  inflight = null
}
