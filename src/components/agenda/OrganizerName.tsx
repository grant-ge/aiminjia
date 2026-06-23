import { useEffect, useState } from 'react'

import { employeeGet } from '@/lib/tauri'

interface OrganizerNameProps {
  employeeId: string
}

export function OrganizerName({ employeeId }: OrganizerNameProps) {
  const normalizedEmployeeId = employeeId.trim()
  const [name, setName] = useState<string | null>(null)
  const [resolved, setResolved] = useState(false)

  useEffect(() => {
    if (!normalizedEmployeeId || normalizedEmployeeId === 'default') {
      setName(null)
      setResolved(true)
      return
    }
    let cancelled = false
    setName(null)
    setResolved(false)
    void employeeGet(normalizedEmployeeId)
      .then((emp) => {
        if (cancelled) return
        if (emp?.name) setName(emp.name)
        setResolved(true)
      })
      .catch(() => {
        if (!cancelled) setResolved(true)
      })
    return () => {
      cancelled = true
    }
  }, [normalizedEmployeeId])

  if (!normalizedEmployeeId || normalizedEmployeeId === 'default') {
    return null
  }

  const displayed = name ?? (resolved ? '未知员工' : normalizedEmployeeId)
  return (
    <span
      className={`text-xs ${name ? 'text-muted-foreground' : 'text-muted-foreground/60'}`}
      title={`员工 ID：${normalizedEmployeeId}`}
    >
      @{displayed}
    </span>
  )
}
