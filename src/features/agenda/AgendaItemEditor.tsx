import { useEffect, useState } from 'react'
import { Folder } from 'lucide-react'

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
  organizerPersonaId: string
  onClose: () => void
  onSaved: () => void
}

const FREQ_NOUN: Record<Freq, string> = {
  daily: '天',
  weekly: '周',
  monthly: '月',
  yearly: '年',
}

export function AgendaItemEditor({
  open,
  initial,
  initialDraft,
  organizerPersonaId,
  onClose,
  onSaved,
}: AgendaItemEditorProps) {
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
  }, [initial, initialDraft, open, homeWorkspace])

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
        const req: CreateAgendaItemRequest = {
          title,
          prompt,
          startAt,
          timezone,
          organizerPersonaId,
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
        title: '选择此日程触发时使用的工作目录',
      })
      if (path) setWorkspacePath(path)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  return (
    <Sheet open={open} onOpenChange={(v) => !v && onClose()}>
      <SheetContent className="w-[480px] flex flex-col gap-4 overflow-y-auto">
        <SheetHeader>
          <SheetTitle>{initial ? '编辑日程' : '新建日程'}</SheetTitle>
        </SheetHeader>

        <Input
          placeholder="标题"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
        />
        <Textarea
          placeholder="到点要做什么？"
          rows={4}
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
        />

        <div className="space-y-2">
          <label className="text-xs text-muted-foreground">频率</label>
          <select
            className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm"
            value={frequency}
            onChange={(e) => setFrequency(e.target.value as Frequency)}
            aria-label="频率"
          >
            <option value="one_shot">一次性</option>
            <option value="daily">每天</option>
            <option value="weekly">每周</option>
            <option value="monthly">每月</option>
            <option value="yearly">每年</option>
          </select>
        </div>

        {frequency !== 'one_shot' ? (
          <div className="space-y-2">
            <label className="text-xs text-muted-foreground">
              {`每 N ${FREQ_NOUN[frequency]}`}
            </label>
            <Input
              type="number"
              min={1}
              value={intervalCount}
              onChange={(e) => setIntervalCount(Math.max(1, Number(e.target.value) || 1))}
              aria-label="间隔"
            />

            <label className="text-xs text-muted-foreground mt-2 block">结束条件</label>
            <select
              className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm"
              value={endKind}
              onChange={(e) => setEndKind(e.target.value as 'never' | 'count' | 'until')}
              aria-label="结束条件"
            >
              <option value="never">永不结束</option>
              <option value="count">N 次后结束</option>
              <option value="until">到日期</option>
            </select>
            {endKind === 'count' ? (
              <Input
                type="number"
                min={1}
                value={endCount}
                onChange={(e) => setEndCount(Math.max(1, Number(e.target.value) || 1))}
                aria-label="结束次数"
              />
            ) : null}
            {endKind === 'until' ? (
              <Input
                type="datetime-local"
                value={endUntilLocal}
                onChange={(e) => setEndUntilLocal(e.target.value)}
                aria-label="结束日期"
              />
            ) : null}
          </div>
        ) : null}

        <div className="space-y-2">
          <label className="text-xs text-muted-foreground" htmlFor="agenda-editor-start">
            开始时间
          </label>
          <Input
            id="agenda-editor-start"
            type="datetime-local"
            value={startAtLocal}
            onChange={(e) => setStartAtLocal(e.target.value)}
          />
        </div>

        <div className="space-y-2">
          <label className="text-xs text-muted-foreground">工作目录</label>
          <div className="flex items-center gap-2">
            <div
              className="flex-1 truncate rounded-md border border-input bg-transparent px-3 py-1.5 text-sm"
              title={workspacePath ?? undefined}
            >
              {workspacePath ?? '使用应用当前工作目录'}
            </div>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={handlePickWorkspace}
              aria-label="选择工作目录"
            >
              <Folder className="h-4 w-4" />
              选择
            </Button>
          </div>
          <p className="text-xs text-muted-foreground">
            触发时会把该目录授权给新对话；留空则跟随应用当前选中的工作目录。
          </p>
        </div>

        {error ? (
          <div className="text-xs text-destructive">{error}</div>
        ) : null}

        <div className="mt-auto flex gap-2">
          <Button variant="outline" onClick={onClose} disabled={saving}>
            取消
          </Button>
          <Button onClick={handleSave} disabled={!canSave}>
            保存
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
