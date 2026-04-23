import { useEffect, useState } from 'react'
import { getArchivedConversations } from '@/lib/tauri'

interface ArchivedConversation {
  id: string
  title: string
  updatedAt: string
  isArchived: boolean
}

export function ArchivedPanel() {
  const [items, setItems] = useState<ArchivedConversation[]>([])
  const [loading, setLoading] = useState(true)

  const load = async () => {
    setLoading(true)
    try {
      const data = await getArchivedConversations()
      setItems(data)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { void load() }, [])

  if (loading) {
    return <div className="text-sm text-muted-foreground p-4">加载中...</div>
  }

  if (items.length === 0) {
    return <div className="text-sm text-muted-foreground p-4">暂无归档记录</div>
  }

  return (
    <div className="flex flex-col gap-2 p-4">
      {items.map((item) => (
        <div key={item.id} className="flex items-center justify-between rounded-lg border border-border bg-card px-4 py-3">
          <div className="flex flex-col gap-0.5">
            <span className="text-sm font-medium text-foreground">{item.title}</span>
            <span className="text-xs text-muted-foreground">
              {new Date(item.updatedAt).toLocaleDateString('zh-CN')}
            </span>
          </div>
        </div>
      ))}
    </div>
  )
}
