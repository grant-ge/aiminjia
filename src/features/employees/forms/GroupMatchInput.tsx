import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Plus, X } from 'lucide-react'

import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'

export interface GroupMatchConfig {
  keywords: string[]
  exclude: string[]
  maxGroups: number
}

interface GroupMatchInputProps {
  value: GroupMatchConfig
  onChange: (next: GroupMatchConfig) => void
  /** Label shown above the component */
  label?: string
  /** Default keyword suggestions shown as placeholder */
  defaultKeywords?: string[]
  /** Default exclude suggestions shown as placeholder */
  defaultExclude?: string[]
}

const DEFAULT_MAX_GROUPS = 50

export function emptyGroupMatch(): GroupMatchConfig {
  return { keywords: [], exclude: [], maxGroups: DEFAULT_MAX_GROUPS }
}

export function groupMatchFromRecord(r: Record<string, unknown>): GroupMatchConfig {
  const gm = r.groupMatch as Record<string, unknown> | undefined
  if (!gm || typeof gm !== 'object') return emptyGroupMatch()
  const keywords = Array.isArray(gm.keywords)
    ? (gm.keywords as unknown[]).filter((k): k is string => typeof k === 'string')
    : []
  const exclude = Array.isArray(gm.exclude)
    ? (gm.exclude as unknown[]).filter((k): k is string => typeof k === 'string')
    : []
  const maxGroups = typeof gm.maxGroups === 'number' ? gm.maxGroups : DEFAULT_MAX_GROUPS
  return { keywords, exclude, maxGroups }
}

export function groupMatchToRecord(gm: GroupMatchConfig): { groupMatch: GroupMatchConfig } {
  return { groupMatch: gm }
}

function TagInput({
  tags,
  onAdd,
  onRemove,
  placeholder,
}: {
  tags: string[]
  onAdd: (tag: string) => void
  onRemove: (index: number) => void
  placeholder: string
}) {
  const [input, setInput] = useState('')

  function handleAdd() {
    const trimmed = input.trim()
    if (trimmed && !tags.includes(trimmed)) {
      onAdd(trimmed)
      setInput('')
    }
  }

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault()
      handleAdd()
    }
  }

  return (
    <div className="flex flex-wrap items-center gap-1.5 rounded-md border border-input bg-background px-2 py-1.5">
      {tags.map((tag, i) => (
        <span
          key={`${tag}-${i}`}
          className="flex items-center gap-0.5 rounded-md bg-accent px-2 py-0.5 text-xs font-medium text-foreground"
        >
          {tag}
          <Button unstyled
            type="button"
            onClick={() => onRemove(i)}
            className="ml-0.5 text-muted-foreground hover:text-destructive"
          >
            <X className="h-3 w-3" />
          </Button>
        </span>
      ))}
      <div className="flex items-center gap-1">
        <Input
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={tags.length === 0 ? placeholder : ''}
          className="h-6 min-w-[80px] flex-1 border-0 bg-transparent p-0 text-xs shadow-none focus-visible:ring-0"
        />
        <Button unstyled
          type="button"
          onClick={handleAdd}
          disabled={!input.trim()}
          className="flex items-center gap-0.5 text-xs text-primary hover:underline disabled:opacity-30"
        >
          <Plus className="h-3 w-3" />
        </Button>
      </div>
    </div>
  )
}

export function GroupMatchInput({
  value,
  onChange,
  label = '',
  defaultKeywords = [],
  defaultExclude = [],
}: GroupMatchInputProps) {
  const { t } = useTranslation()

  function update(patch: Partial<GroupMatchConfig>) {
    onChange({ ...value, ...patch })
  }

  const eg = t('employee.config.groupMatch.eg')

  const keywordsPlaceholder =
    defaultKeywords.length > 0
      ? `${eg} ${defaultKeywords.join(', ')}`
      : ''

  const excludePlaceholder =
    defaultExclude.length > 0
      ? `${eg} ${defaultExclude.join(', ')}`
      : ''

  return (
    <div className="flex flex-col gap-3">
      {label && (
        <label className="text-xs font-medium text-muted-foreground">{label}</label>
      )}

      <div className="flex flex-col gap-1.5">
        <label className="text-xs text-muted-foreground">
          <span className="font-medium">{t('employee.config.groupMatch.includeLabel')}</span>
        </label>
        <TagInput
          tags={value.keywords}
          onAdd={(tag) => update({ keywords: [...value.keywords, tag] })}
          onRemove={(i) => update({ keywords: value.keywords.filter((_, idx) => idx !== i) })}
          placeholder={keywordsPlaceholder}
        />
        <p className="text-xs text-[rgba(var(--muted-foreground-rgb),0.70)]">
          {t('employee.config.groupMatch.includeHintSimple')}
        </p>
      </div>

      <div className="flex flex-col gap-1.5">
        <label className="text-xs text-muted-foreground">
          <span className="font-medium">{t('employee.config.groupMatch.excludeLabel')}</span>
        </label>
        <TagInput
          tags={value.exclude}
          onAdd={(tag) => update({ exclude: [...value.exclude, tag] })}
          onRemove={(i) => update({ exclude: value.exclude.filter((_, idx) => idx !== i) })}
          placeholder={excludePlaceholder}
        />
      </div>

      <div className="flex items-center gap-2">
        <label className="text-xs text-muted-foreground whitespace-nowrap">
          {t('employee.config.groupMatch.maxGroupsLabel')}
        </label>
        <Input
          type="number"
          value={value.maxGroups}
          onChange={(e) => update({ maxGroups: Math.max(1, Math.min(200, Number(e.target.value) || DEFAULT_MAX_GROUPS)) })}
          className="h-7 w-20 text-xs"
          min={1}
          max={200}
        />
      </div>
    </div>
  )
}
