import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

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
  const { t, i18n } = useTranslation()
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
              <TabsTrigger value="overview">{t('schedules.detail.tabs.overview')}</TabsTrigger>
              <TabsTrigger value="history">{t('schedules.detail.tabs.history')}</TabsTrigger>
              <TabsTrigger value="settings">{t('schedules.detail.tabs.settings')}</TabsTrigger>
            </TabsList>
            <TabsContent value="overview" className="space-y-2">
              <Row
                label={t('schedules.detail.labels.organizer')}
                value={item.organizerEmployeeId}
              />
              <Row
                label={t('schedules.detail.labels.frequency')}
                value={describeFrequency(item.rule, item.startAt, item.timezone, t, i18n.language)}
              />
              <Row label={t('schedules.detail.labels.nextFire')} value={item.nextFireAt ?? '-'} />
              <Row
                label={t('schedules.detail.labels.status')}
                value={t(`schedules.row.status.${item.status}`)}
              />
              <Row
                label={t('schedules.detail.labels.workspace')}
                value={item.workspacePath ?? t('schedules.detail.workspaceDefault')}
              />
            </TabsContent>
            <TabsContent value="history" className="space-y-1">
              {occs.length === 0 ? (
                <div className="text-xs text-muted-foreground">
                  {t('schedules.detail.historyEmpty')}
                </div>
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
              <Button onClick={() => setEditorOpen(true)}>
                {t('schedules.detail.editButton')}
              </Button>
            </TabsContent>
          </Tabs>
        </SheetContent>
      </Sheet>
      <AgendaItemEditor
        open={editorOpen}
        initial={item}
        organizerEmployeeId={item.organizerEmployeeId}
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
