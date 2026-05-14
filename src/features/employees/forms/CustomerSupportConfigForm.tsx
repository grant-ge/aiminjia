import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { GroupMatchInput, groupMatchFromRecord, groupMatchToRecord, type GroupMatchConfig } from './GroupMatchInput'
import { KnowledgeSourcesField, parseKnowledgeSources, type KnowledgeSource } from './KnowledgeSourcesField'

interface Props {
  initial: Record<string, unknown>
  onSubmit: (next: Record<string, unknown>) => void
  onCancel: () => void
}

type ResponseStyle = 'professional' | 'friendly' | 'concise'
type SummaryCron = 'daily' | 'weekly' | 'off'

interface FormState {
  groupMatch: GroupMatchConfig
  responseStyle: ResponseStyle
  greeting: string
  closing: string
  summaryCron: SummaryCron
  knowledgeSources: KnowledgeSource[]
  escalationKeywords: string[]
  techKeywords: string[]
}

const DEFAULT_ESCALATION = ['投诉', '退款', '赔偿', '律师', '工信部']
const DEFAULT_TECH = ['报错', 'error', '500', 'API', '部署', '配置', '日志', 'bug']

function parseStringArray(v: unknown): string[] {
  if (!Array.isArray(v)) return []
  return (v as unknown[]).filter((x): x is string => typeof x === 'string')
}

function stateFromInitial(initial: Record<string, unknown>): FormState {
  const gm = groupMatchFromRecord(initial)
  if (gm.keywords.length === 0) {
    gm.keywords = ['服务', '客户', '售后']
    gm.exclude = ['内部', '测试']
  }
  const responseStyle = (['professional', 'friendly', 'concise'].includes(initial.responseStyle as string)
    ? initial.responseStyle : 'friendly') as ResponseStyle
  const summaryCron = (['daily', 'weekly', 'off'].includes(initial.summaryCron as string)
    ? initial.summaryCron : 'weekly') as SummaryCron
  return {
    groupMatch: gm,
    responseStyle,
    greeting: typeof initial.greeting === 'string' ? initial.greeting : '您好，',
    closing: typeof initial.closing === 'string' ? initial.closing : '如还有其他问题随时告诉我们~',
    summaryCron,
    knowledgeSources: parseKnowledgeSources(initial.knowledgeSources),
    escalationKeywords: parseStringArray(initial.escalationKeywords).length > 0
      ? parseStringArray(initial.escalationKeywords) : DEFAULT_ESCALATION,
    techKeywords: parseStringArray(initial.techKeywords).length > 0
      ? parseStringArray(initial.techKeywords) : DEFAULT_TECH,
  }
}

function InlineTagEditor({ label, hint, tags, onChange }: { label: string; hint: string; tags: string[]; onChange: (n: string[]) => void }) {
  const [input, setInput] = useState('')
  return (
    <div className="flex flex-col gap-1.5">
      <label className="text-xs font-medium text-muted-foreground">{label}</label>
      <div className="flex flex-wrap items-center gap-1.5">
        {tags.map((tag, i) => (
          <span key={`${tag}-${i}`} className="flex items-center gap-0.5 rounded-md bg-accent px-2 py-0.5 text-xs">
            {tag}
            <button type="button" onClick={() => onChange(tags.filter((_, idx) => idx !== i))} className="ml-0.5 text-xs text-muted-foreground hover:text-destructive">×</button>
          </span>
        ))}
        <input
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault()
              const t = input.trim()
              if (t && !tags.includes(t)) { onChange([...tags, t]); setInput('') }
            }
          }}
          placeholder="+"
          className="h-6 w-16 rounded border border-input bg-background px-1 text-xs"
        />
      </div>
      <p className="text-xs text-muted-foreground/70">{hint}</p>
    </div>
  )
}

export function CustomerSupportConfigForm({ initial, onSubmit, onCancel }: Props) {
  const { t } = useTranslation()
  const [state, setState] = useState<FormState>(() => stateFromInitial(initial))

  function update(patch: Partial<FormState>) { setState((s) => ({ ...s, ...patch })) }

  function handleSave() {
    onSubmit({
      ...groupMatchToRecord(state.groupMatch),
      responseStyle: state.responseStyle,
      greeting: state.greeting,
      closing: state.closing,
      escalationKeywords: state.escalationKeywords,
      techKeywords: state.techKeywords,
      summaryCron: state.summaryCron,
      knowledgeSources: state.knowledgeSources,
      language: 'zh',
    })
  }

  const styles: ResponseStyle[] = ['professional', 'friendly', 'concise']
  const summaries: SummaryCron[] = ['daily', 'weekly', 'off']

  return (
    <div className="flex flex-col gap-4">
      <p className="text-xs leading-relaxed text-muted-foreground">{t('employee.config.customerSupport.intro')}</p>

      <GroupMatchInput
        value={state.groupMatch}
        onChange={(gm) => update({ groupMatch: gm })}
        defaultKeywords={['服务', '客户', '售后']}
        defaultExclude={['内部', '测试']}
      />

      <KnowledgeSourcesField
        value={state.knowledgeSources}
        onChange={(next) => update({ knowledgeSources: next })}
      />

      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">{t('employee.config.responseStyle.label')}</label>
        <div className="flex items-center gap-3 text-sm">
          {styles.map((opt) => (
            <label key={opt} className="flex items-center gap-1.5">
              <input type="radio" checked={state.responseStyle === opt} onChange={() => update({ responseStyle: opt })} />
              {t(`employee.config.responseStyle.${opt}`)}
            </label>
          ))}
        </div>
      </div>

      <div className="flex items-center gap-3">
        <div className="flex flex-1 flex-col gap-1.5">
          <label className="text-xs font-medium text-muted-foreground">{t('employee.config.greeting')}</label>
          <Input value={state.greeting} onChange={(e) => update({ greeting: e.target.value })} className="text-xs" />
        </div>
        <div className="flex flex-1 flex-col gap-1.5">
          <label className="text-xs font-medium text-muted-foreground">{t('employee.config.closing')}</label>
          <Input value={state.closing} onChange={(e) => update({ closing: e.target.value })} className="text-xs" />
        </div>
      </div>

      <InlineTagEditor
        label={t('employee.config.escalation.label')}
        hint={t('employee.config.escalation.hint')}
        tags={state.escalationKeywords}
        onChange={(next) => update({ escalationKeywords: next })}
      />
      <InlineTagEditor
        label={t('employee.config.techKeywords.label')}
        hint={t('employee.config.techKeywords.hint')}
        tags={state.techKeywords}
        onChange={(next) => update({ techKeywords: next })}
      />

      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">{t('employee.config.summaryFreq.label')}</label>
        <div className="flex items-center gap-3 text-sm">
          {summaries.map((opt) => (
            <label key={opt} className="flex items-center gap-1.5">
              <input type="radio" checked={state.summaryCron === opt} onChange={() => update({ summaryCron: opt })} />
              {t(`employee.config.summaryFreq.${opt}`)}
            </label>
          ))}
        </div>
      </div>

      <div className="flex items-center justify-end gap-2 pt-2">
        <Button variant="ghost" onClick={onCancel}>{t('employee.config.cancel')}</Button>
        <Button onClick={handleSave}>{t('employee.config.save')}</Button>
      </div>
    </div>
  )
}
