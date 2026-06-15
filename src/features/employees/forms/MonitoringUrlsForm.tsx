import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Plus, Trash2 } from 'lucide-react'

import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'

interface Row {
  name: string
  url: string
  tagsRaw: string  // comma-separated input; normalized on submit
}

interface MonitoringTarget {
  name: string
  url: string
  tags: string[]
}

interface MonitoringUrlsFormProps {
  initial: Record<string, unknown>
  onSubmit: (next: { monitoringTargets: MonitoringTarget[] }) => void
  onCancel: () => void
}

function rowsFromInitial(initial: Record<string, unknown>): Row[] {
  const arr = initial.monitoringTargets
  if (!Array.isArray(arr) || arr.length === 0) {
    return [{ name: '', url: '', tagsRaw: '' }]
  }
  return arr.map((it) => {
    const item = it as { name?: string; url?: string; tags?: string[] }
    return {
      name: item.name ?? '',
      url: item.url ?? '',
      tagsRaw: (item.tags ?? []).join(', '),
    }
  })
}

export function MonitoringUrlsForm({ initial, onSubmit, onCancel }: MonitoringUrlsFormProps) {
  const { t } = useTranslation()
  const [rows, setRows] = useState<Row[]>(() => rowsFromInitial(initial))

  function update(i: number, patch: Partial<Row>) {
    setRows((rs) => rs.map((r, idx) => (idx === i ? { ...r, ...patch } : r)))
  }

  function addRow() {
    setRows((rs) => [...rs, { name: '', url: '', tagsRaw: '' }])
  }

  function removeRow(i: number) {
    setRows((rs) => (rs.length <= 1 ? rs : rs.filter((_, idx) => idx !== i)))
  }

  function handleSave() {
    const monitoringTargets: MonitoringTarget[] = rows
      .map((r) => ({
        name: r.name.trim(),
        url: r.url.trim(),
        tags: r.tagsRaw
          .split(',')
          .map((t) => t.trim())
          .filter(Boolean),
      }))
      .filter((r) => r.name || r.url || r.tags.length > 0)
    onSubmit({ monitoringTargets })
  }

  return (
    <div data-aijia-resource-form="monitoring-urls" className="flex flex-col gap-4">
      <div className="flex flex-col gap-2">
        {rows.map((r, i) => (
          <div key={i} data-aijia-resource-row={i} className="flex items-start gap-2">
            <Input
              placeholder={t('employee.config.monitoringUrls.namePlaceholder')}
              value={r.name}
              onChange={(e) => update(i, { name: e.target.value })}
              data-aijia-resource-field="name"
              className="w-40"
            />
            <Input
              placeholder={t('employee.config.monitoringUrls.urlPlaceholder')}
              value={r.url}
              onChange={(e) => update(i, { url: e.target.value })}
              data-aijia-resource-field="url"
              className="flex-1 font-mono text-xs"
            />
            <Input
              placeholder={t('employee.config.monitoringUrls.tagsPlaceholder')}
              value={r.tagsRaw}
              onChange={(e) => update(i, { tagsRaw: e.target.value })}
              data-aijia-resource-field="tags"
              className="w-44"
            />
            <Button unstyled
              type="button"
              onClick={() => removeRow(i)}
              data-aijia-resource-action="remove-row"
              disabled={rows.length <= 1}
              className="p-2 text-muted-foreground hover:text-destructive disabled:opacity-30"
            >
              <Trash2 className="h-4 w-4" />
            </Button>
          </div>
        ))}
      </div>

      <Button unstyled
        type="button"
        onClick={addRow}
        data-aijia-resource-action="add-row"
        className="flex items-center gap-1 self-start text-xs text-primary hover:underline"
      >
        <Plus className="h-3 w-3" /> {t('employee.config.monitoringUrls.addRow')}
      </Button>

      <div className="flex items-center justify-end gap-2 pt-2">
        <Button variant="ghost" data-aijia-resource-action="cancel" onClick={onCancel}>
          {t('employee.config.cancel')}
        </Button>
        <Button data-aijia-resource-action="save" onClick={handleSave}>
          {t('employee.config.save')}
        </Button>
      </div>
    </div>
  )
}
