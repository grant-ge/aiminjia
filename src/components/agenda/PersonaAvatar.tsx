import { useEffect, useState } from 'react'

import { invoke } from '@tauri-apps/api/core'

interface PersonaAvatarProps {
  personaId: string
  size?: 'sm' | 'md'
}

export function PersonaAvatar({ personaId, size = 'md' }: PersonaAvatarProps) {
  const [name, setName] = useState<string>(personaId)

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const persona = await invoke<{ name?: string }>('get_persona', { id: personaId })
        if (!cancelled && persona?.name) {
          setName(persona.name)
        }
      } catch {
        // 取不到 persona（已删 / 权限问题）就退化成显示 personaId 首字
      }
    })()
    return () => {
      cancelled = true
    }
  }, [personaId])

  const dim = size === 'sm' ? 'w-5 h-5 text-[10px]' : 'w-7 h-7 text-xs'
  const initial = (name || personaId || '?').slice(0, 1)

  return (
    <div
      className={`rounded-full bg-muted flex items-center justify-center ${dim}`}
      title={name}
      aria-label={`组织者 ${name}`}
    >
      {initial}
    </div>
  )
}
