import { useEffect, useState } from 'react'

import { listAgents, type AgentInfo } from '@/lib/tauri'

interface AgentSelectorProps {
  value: string | null
  onChange: (agentName: string | null) => void
}

export function AgentSelector({ value, onChange }: AgentSelectorProps) {
  const [agents, setAgents] = useState<AgentInfo[]>([])

  useEffect(() => {
    let active = true

    listAgents()
      .then((result) => {
        if (active) {
          setAgents(result)
        }
      })
      .catch((err) => {
        console.error('[AgentSelector] failed to load agents', err)
      })

    return () => {
      active = false
    }
  }, [])

  return (
    <select
      aria-label="Agent selector"
      className="h-8 max-w-[180px] shrink-0 rounded-lg border-none px-2 text-sm outline-none"
      style={{
        background: 'var(--color-bg-card)',
        color: 'var(--color-text-primary)',
      }}
      value={value ?? ''}
      onChange={(event) => onChange(event.target.value || null)}
    >
      <option value="">自动（默认）</option>
      {agents.map((agent) => (
        <option key={agent.name} value={agent.name}>
          {agent.description || agent.name}
        </option>
      ))}
    </select>
  )
}
