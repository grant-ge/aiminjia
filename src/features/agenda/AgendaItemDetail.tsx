import { useEffect, useState } from 'react'

import {
  type AgendaItem,
  type Occurrence,
  listAgendaOccurrences,
} from '@/lib/tauri'
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Button } from '@/components/ui/button'

import { AgendaItemEditor } from './AgendaItemEditor'
import { describeFrequency } from './describeFrequency'

interface AgendaItemDetailProps {
  open: boolean
  item: AgendaItem | null
  onClose: () => void
  onChanged: () => void
}

export function AgendaItemDetail({
  open,
  item,
  onClose,
  onChanged,
}: AgendaItemDetailProps) {
  const [occs, setOccs] = useState<Occurrence[]>([])
  const [editorOpen, setEditorOpen] = useState(false)

  useEffect(() => {
    if (!item) return
    let cancelled = false
    void (async () => {
      try {
        const list = await listAgendaOccurrences(item.id, 50)
        if (!cancelled) setOccs(list)
      } catch {
        if (!cancelled) setOccs([])
      }
    })()
    return () => {
      cancelled = true
    }
  }, [item])

  if (!item) return null

  return (
    <>
      <Sheet open={open} onOpenChange={(v) => !v && onClose()}>
        <SheetContent className="w-[520px] flex flex-col gap-4 overflow-y-auto">
          <SheetHeader>
            <SheetTitle>{item.title}</SheetTitle>
          </SheetHeader>
          <Tabs defaultValue="overview" className="flex flex-col gap-3">
            <TabsList>
              <TabsTrigger value="overview">概览</TabsTrigger>
              <TabsTrigger value="history">执行历史</TabsTrigger>
              <TabsTrigger value="settings">设置</TabsTrigger>
            </TabsList>
            <TabsContent value="overview" className="space-y-2">
              <Row label="组织者" value={item.organizerPersonaId} />
              <Row
                label="频率"
                value={describeFrequency(item.rule, item.startAt, item.timezone)}
              />
              <Row label="下次触发" value={item.nextFireAt ?? '-'} />
              <Row label="状态" value={item.status} />
              <Row label="工作目录" value={item.workspacePath ?? '使用应用当前目录'} />
            </TabsContent>
            <TabsContent value="history" className="space-y-1">
              {occs.length === 0 ? (
                <div className="text-xs text-muted-foreground">暂无执行记录</div>
              ) : (
                occs.map((o) => (
                  <div
                    key={o.id}
                    className="flex items-center gap-2 border-b py-1 text-sm"
                  >
                    <span className="text-muted-foreground">{o.firedAt}</span>
                    <span className={statusColorClass(o.status)}>{o.status}</span>
                    <span className="flex-1 truncate text-xs">
                      {o.errorSummary ?? ''}
                    </span>
                  </div>
                ))
              )}
            </TabsContent>
            <TabsContent value="settings">
              <Button onClick={() => setEditorOpen(true)}>编辑</Button>
            </TabsContent>
          </Tabs>
        </SheetContent>
      </Sheet>
      <AgendaItemEditor
        open={editorOpen}
        initial={item}
        organizerPersonaId={item.organizerPersonaId}
        onClose={() => setEditorOpen(false)}
        onSaved={() => {
          onChanged()
          setEditorOpen(false)
        }}
      />
    </>
  )
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex gap-2 text-sm">
      <span className="w-20 text-muted-foreground">{label}</span>
      <span>{value}</span>
    </div>
  )
}

function statusColorClass(status: Occurrence['status']) {
  if (status === 'succeeded') return 'text-green-600'
  if (status === 'failed') return 'text-red-600'
  return 'text-yellow-600'
}
