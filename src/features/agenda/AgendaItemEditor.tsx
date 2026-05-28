import { useEffect, useState } from 'react'
import { Folder } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import {
  type AgendaItem,
  type CreateAgendaItemRequest,
  type Freq,
  type RecurrenceRule,
  type UpdateAgendaItemRequest,
  createAgendaItem,
  getDefaultFolder,
  pickLocalDirectory,
  updateAgendaItem,
} from '@/lib/tauri'
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { useHomeStore } from '@/stores/homeStore'

type Frequency = 'one_shot' | Freq

interface AgendaItemEditorProps {
  open: boolean
  initial?: AgendaItem | null
  initialDraft?: Partial<CreateAgendaItemRequest> | null
  organizerEmployeeId?: string
  onClose: () => void
  onSaved: () => void
}

const DEFAULT_AGENDA_ORGANIZER_ID = 'default'

export function AgendaItemEditor({
  open,
  initial,
  initialDraft,
  organizerEmployeeId,
  onClose,
  onSaved,
}: AgendaItemEditorProps) {
  const { t } = useTranslation()
  const [title, setTitle] = useState('')
  const [prompt, setPrompt] = useState('')
  const [startAtLocal, setStartAtLocal] = useState('')
  const [timezone, setTimezone] = useState('Asia/Shanghai')
  const [frequency, setFrequency] = useState<Frequency>('one_shot')
  const [intervalCount, setIntervalCount] = useState(1)
  const [endKind, setEndKind] = useState<'never' | 'count' | 'until'>('never')
  const [endCount, setEndCount] = useState(10)
  const [endUntilLocal, setEndUntilLocal] = useState('')
  const [workspacePath, setWorkspacePath] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const homeWorkspace = useHomeStore((s) => s.selectedWorkspace)

  useEffect(() => {
    if (initial) {
      setTitle(initial.title)
      setPrompt(initial.prompt)
      setStartAtLocal(toLocalInput(initial.startAt))
      setTimezone(initial.timezone)
      setFrequency(initial.rule?.freq ?? 'one_shot')
      setIntervalCount(initial.rule?.interval ?? 1)
      const ec = initial.rule?.endCondition
      if (!ec || ec.kind === 'never') {
        setEndKind('never')
      } else if (ec.kind === 'count') {
        setEndKind('count')
        setEndCount(ec.n)
      } else {
        setEndKind('until')
        setEndUntilLocal(toLocalInput(ec.at))
      }
      setWorkspacePath(initial.workspacePath ?? null)
    } else {
      setTitle(initialDraft?.title ?? '')
      setPrompt(initialDraft?.prompt ?? '')
      setStartAtLocal('')
      setTimezone(initialDraft?.timezone ?? 'Asia/Shanghai')
      const draftRule = initialDraft?.rule ?? null
      setFrequency(draftRule?.freq ?? 'one_shot')
      setIntervalCount(draftRule?.interval ?? 1)
      const ec = draftRule?.endCondition
      if (!ec || ec.kind === 'never') {
        setEndKind('never')
      } else if (ec.kind === 'count') {
        setEndKind('count')
        setEndCount(ec.n)
      } else {
        setEndKind('until')
        setEndUntilLocal(toLocalInput(ec.at))
      }
      // 新建：默认用 home picker 选过的 workspace；没选过就异步取 default folder
      if (initialDraft?.workspacePath !== undefined) {
        setWorkspacePath(initialDraft.workspacePath ?? null)
      } else if (homeWorkspace?.rootPath) {
        setWorkspacePath(homeWorkspace.rootPath)
      } else {
        setWorkspacePath(null)
        getDefaultFolder()
          .then((ws) => {
            if (ws?.rootPath) {
              setWorkspacePath((prev) => prev ?? ws.rootPath)
            }
          })
          .catch(() => {
            // 非致命：保留 null，后端会 fallback 到全局 workspace
          })
      }
    }
    setError(null)
  }, [initial, initialDraft, open, homeWorkspace, organizerEmployeeId])

  const buildRule = (): RecurrenceRule | null => {
    if (frequency === 'one_shot') return null
    let endCondition: RecurrenceRule['endCondition']
    if (endKind === 'never') {
      endCondition = { kind: 'never' }
    } else if (endKind === 'count') {
      endCondition = { kind: 'count', n: endCount }
    } else {
      endCondition = { kind: 'until', at: new Date(endUntilLocal).toISOString() }
    }
    return { freq: frequency, interval: intervalCount, endCondition }
  }

  const canSave = !!title && !!prompt && !!startAtLocal && !saving

  const handleSave = async () => {
    setSaving(true)
    setError(null)
    try {
      const startAt = new Date(startAtLocal).toISOString()
      if (initial) {
        const req: UpdateAgendaItemRequest = {
          title,
          prompt,
          startAt,
          timezone,
          rule: buildRule(),
          workspacePath,
        }
        await updateAgendaItem(initial.id, req)
      } else {
        const organizerId = (organizerEmployeeId ?? '').trim() || DEFAULT_AGENDA_ORGANIZER_ID
        const req: CreateAgendaItemRequest = {
          title,
          prompt,
          startAt,
          timezone,
          organizerEmployeeId: organizerId,
          rule: buildRule(),
          workspacePath,
        }
        await createAgendaItem(req)
      }
      onSaved()
      onClose()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  const handlePickWorkspace = async () => {
    try {
      const path = await pickLocalDirectory({
        defaultPath: workspacePath ?? undefined,
        title: t('schedules.editor.fields.pickDialogTitle'),
      })
      if (path) setWorkspacePath(path)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  return (
    <Sheet open={open} onOpenChange={(v) => !v && onClose()}>
      <SheetContent
        data-aijia-agenda-editor
        className="w-[480px] flex flex-col gap-4 overflow-y-auto"
      >
        <SheetHeader>
          <SheetTitle>
            {initial ? t('schedules.editor.titleEdit') : t('schedules.editor.titleNew')}
          </SheetTitle>
        </SheetHeader>

        <Input
          placeholder={t('schedules.editor.fields.title')}
          aria-label={t('schedules.editor.fields.title')}
          data-aijia-agenda-field="title"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
        />
        <Textarea
          placeholder={t('schedules.editor.fields.promptPlaceholder')}
          aria-label={t('schedules.editor.fields.promptAria')}
          data-aijia-agenda-field="prompt"
          rows={4}
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
        />

        <div className="space-y-2">
          <label className="text-xs text-muted-foreground">
            {t('schedules.editor.fields.frequency')}
          </label>
          <select
            className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm"
            value={frequency}
            onChange={(e) => setFrequency(e.target.value as Frequency)}
            aria-label={t('schedules.editor.fields.frequency')}
          >
            <option value="one_shot">{t('schedules.editor.freqOptions.oneShot')}</option>
            <option value="daily">{t('schedules.editor.freqOptions.daily')}</option>
            <option value="weekly">{t('schedules.editor.freqOptions.weekly')}</option>
            <option value="monthly">{t('schedules.editor.freqOptions.monthly')}</option>
            <option value="yearly">{t('schedules.editor.freqOptions.yearly')}</option>
          </select>
        </div>

        {frequency !== 'one_shot' ? (
          <div className="space-y-2">
            <label className="text-xs text-muted-foreground">
              {t('schedules.editor.fields.intervalEvery', {
                unit: t(`schedules.frequency.noun.${frequency}`),
              })}
            </label>
            <Input
              type="number"
              min={1}
              value={intervalCount}
              onChange={(e) => setIntervalCount(Math.max(1, Number(e.target.value) || 1))}
              aria-label={t('schedules.editor.fields.intervalAria')}
            />

            <label className="text-xs text-muted-foreground mt-2 block">
              {t('schedules.editor.fields.endCondition')}
            </label>
            <select
              className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm"
              value={endKind}
              onChange={(e) => setEndKind(e.target.value as 'never' | 'count' | 'until')}
              aria-label={t('schedules.editor.fields.endCondition')}
            >
              <option value="never">{t('schedules.editor.endOptions.never')}</option>
              <option value="count">{t('schedules.editor.endOptions.count')}</option>
              <option value="until">{t('schedules.editor.endOptions.until')}</option>
            </select>
            {endKind === 'count' ? (
              <Input
                type="number"
                min={1}
                value={endCount}
                onChange={(e) => setEndCount(Math.max(1, Number(e.target.value) || 1))}
                aria-label={t('schedules.editor.fields.endCountAria')}
              />
            ) : null}
            {endKind === 'until' ? (
              <Input
                type="datetime-local"
                value={endUntilLocal}
                onChange={(e) => setEndUntilLocal(e.target.value)}
                aria-label={t('schedules.editor.fields.endUntilAria')}
              />
            ) : null}
          </div>
        ) : null}

        <div className="space-y-2">
          <label className="text-xs text-muted-foreground" htmlFor="agenda-editor-start">
            {t('schedules.editor.fields.startTime')}
          </label>
          <Input
            id="agenda-editor-start"
            type="datetime-local"
            value={startAtLocal}
            onChange={(e) => setStartAtLocal(e.target.value)}
          />
        </div>

        <div className="space-y-2">
          <label className="text-xs text-muted-foreground">
            {t('schedules.editor.fields.workspace')}
          </label>
          <div className="flex items-center gap-2">
            <div
              className="flex-1 truncate rounded-md border border-input bg-transparent px-3 py-1.5 text-sm"
              title={workspacePath ?? undefined}
            >
              {workspacePath ?? t('schedules.editor.fields.workspaceDefault')}
            </div>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={handlePickWorkspace}
              aria-label={t('schedules.editor.fields.pickWorkspaceAria')}
            >
              <Folder className="h-4 w-4" />
              {t('schedules.editor.fields.pick')}
            </Button>
          </div>
          <p className="text-xs text-muted-foreground">
            {t('schedules.editor.fields.workspaceHint')}
          </p>
        </div>

        {error ? (
          <div className="text-xs text-destructive">{error}</div>
        ) : null}

        <div className="mt-auto flex gap-2">
          <Button variant="outline" onClick={onClose} disabled={saving} data-aijia-agenda-action="cancel">
            {t('schedules.editor.actions.cancel')}
          </Button>
          <Button onClick={handleSave} disabled={!canSave} data-aijia-agenda-action="save">
            {t('schedules.editor.actions.save')}
          </Button>
        </div>
      </SheetContent>
    </Sheet>
  )
}

function toLocalInput(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return ''
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`
}
