import { useEffect, useState } from 'react'

import { employeeGet } from '@/lib/tauri'

interface OrganizerNameProps {
  employeeId: string
}

export function OrganizerName({ employeeId }: OrganizerNameProps) {
  const [name, setName] = useState<string | null>(null)
  const [resolved, setResolved] = useState(false)

  useEffect(() => {
    let cancelled = false
    setName(null)
    setResolved(false)
    void employeeGet(employeeId)
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
  }, [employeeId])

  const displayed = name ?? (resolved ? '未知员工' : employeeId)
  return (
    <span
      className={`text-xs ${name ? 'text-muted-foreground' : 'text-muted-foreground/60'}`}
      title={`员工 ID：${employeeId}`}
    >
      @{displayed}
    </span>
  )
}
