import { open } from '@tauri-apps/plugin-dialog'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'

export type KnowledgeSourceStatus = 'pending' | 'indexing' | 'done' | 'failed'

export interface KnowledgeSource {
  path: string
  originalName: string
  size: number
  status: KnowledgeSourceStatus
  slicedCount: number
  error?: string
}

interface Props {
  value: KnowledgeSource[]
  onChange: (next: KnowledgeSource[]) => void
  onRetry?: (source: KnowledgeSource) => void
}

export function KnowledgeSourcesField({ value, onChange, onRetry }: Props) {
  const { t } = useTranslation()

  async function pickFiles() {
    const selected = await open({
      multiple: true,
      filters: [{ name: 'Knowledge', extensions: ['md', 'txt', 'pdf', 'docx'] }],
    })
    if (!selected) return
    const arr = Array.isArray(selected) ? selected : [selected]
    const additions: KnowledgeSource[] = arr.map((p) => ({
      path: p,
      originalName: p.split(/[\\/]/).pop() ?? p,
      size: 0,
      status: 'pending',
      slicedCount: 0,
    }))
    onChange([...value, ...additions])
  }

  function remove(idx: number) {
    onChange(value.filter((_, i) => i !== idx))
  }

  function statusLabel(s: KnowledgeSource): string {
    switch (s.status) {
      case 'pending': return t('employee.config.knowledge.statusPending')
      case 'indexing': return t('employee.config.knowledge.statusIndexing')
      case 'done': return t('employee.config.knowledge.statusDone', { count: s.slicedCount })
      case 'failed': return t('employee.config.knowledge.statusFailed')
    }
  }

  return (
    <div className="flex flex-col gap-1.5">
      <label className="text-xs font-medium text-muted-foreground">
        {t('employee.config.knowledge.label')}
      </label>
      <p className="text-xs text-muted-foreground/70">
        {t('employee.config.knowledge.hint')}
      </p>
      <div className="flex flex-col gap-1">
        {value.map((s, i) => (
          <div key={`${s.path}-${i}`} className="flex items-center gap-2 rounded border border-input bg-background px-2 py-1 text-xs">
            <span className="flex-1 truncate">📄 {s.originalName}</span>
            <span
              className={
                s.status === 'failed'
                  ? 'text-destructive'
                  : s.status === 'done'
                    ? 'text-green-600'
                    : 'text-muted-foreground'
              }
              title={s.error}
            >
              {statusLabel(s)}
            </span>
            {s.status === 'failed' && onRetry && (
              <button type="button" onClick={() => onRetry(s)} className="text-blue-600 hover:underline">
                {t('employee.config.knowledge.retry')}
              </button>
            )}
            <button type="button" onClick={() => remove(i)} className="text-muted-foreground hover:text-destructive">
              ×
            </button>
          </div>
        ))}
      </div>
      <Button type="button" variant="outline" size="sm" onClick={pickFiles} className="w-fit">
        + {t('employee.config.knowledge.upload')}
      </Button>
    </div>
  )
}
