import { useCallback, useEffect, useState } from 'react'
import { employeeList, type EmployeeRecord } from '@/lib/tauri'

export function useEmployees() {
  const [employees, setEmployees] = useState<EmployeeRecord[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setError(null)
    const items = await employeeList()
    setEmployees(items)
  }, [])

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    refresh()
      .catch((err) => {
        if (!cancelled) setError(String(err))
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => { cancelled = true }
  }, [refresh])

  return { employees, loading, error, refresh }
}
