import { useState } from 'react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { GroupMatchInput, groupMatchFromRecord, groupMatchToRecord, type GroupMatchConfig } from './GroupMatchInput'

interface CustomerSupportConfigFormProps {
  initial: Record<string, unknown>
  onSubmit: (next: Record<string, unknown>) => void
  onCancel: () => void
}

type ResponseStyle = 'professional' | 'friendly' | 'concise'
type SummaryCron = 'daily' | 'weekly' | 'off'

interface TagListState {
  escalationKeywords: string[]
  techKeywords: string[]
}

interface FormState {
  groupMatch: GroupMatchConfig
  responseStyle: ResponseStyle
  greeting: string
  closing: string
  summaryCron: SummaryCron
  tags: TagListState
}

const RESPONSE_STYLES: { value: ResponseStyle; label: string }[] = [
  { value: 'professional', label: 'Professional' },
  { value: 'friendly', label: 'Friendly' },
  { value: 'concise', label: 'Concise' },
]

const SUMMARY_OPTIONS: { value: SummaryCron; label: string }[] = [
  { value: 'daily', label: 'Daily' },
  { value: 'weekly', label: 'Weekly' },
  { value: 'off', label: 'Off' },
]

const DEFAULT_ESCALATION = ['投诉', '退款', '赔偿', '律师', '工信部']
const DEFAULT_TECH = ['报错', 'error', '500', 'API', '部署', '配置', '日志', 'bug']

function parseStringArray(val: unknown): string[] {
  if (!Array.isArray(val)) return []
  return (val as unknown[]).filter((k): k is string => typeof k === 'string')
}

function stateFromInitial(initial: Record<string, unknown>): FormState {
  const gm = groupMatchFromRecord(initial)
  if (gm.keywords.length === 0) {
    gm.keywords = ['服务', '客户', '售后']
    gm.exclude = ['内部', '测试']
  }
  const responseStyle = (['professional', 'friendly', 'concise'].includes(initial.responseStyle as string)
    ? initial.responseStyle
    : 'friendly') as ResponseStyle
  const greeting = typeof initial.greeting === 'string' ? initial.greeting : '您好，'
  const closing = typeof initial.closing === 'string' ? initial.closing : '如还有其他问题随时告诉我们~'
  const summaryCron = (['daily', 'weekly', 'off'].includes(initial.summaryCron as string)
    ? initial.summaryCron
    : 'weekly') as SummaryCron
  const escalationRaw = parseStringArray(initial.escalationKeywords)
  const techRaw = parseStringArray(initial.techKeywords)
  return {
    groupMatch: gm,
    responseStyle,
    greeting,
    closing,
    summaryCron,
    tags: {
      escalationKeywords: escalationRaw.length > 0 ? escalationRaw : DEFAULT_ESCALATION,
      techKeywords: techRaw.length > 0 ? techRaw : DEFAULT_TECH,
    },
  }
}

function InlineTagEditor({
  label,
  hint,
  tags,
  onChange,
}: {
  label: string
  hint: string
  tags: string[]
  onChange: (next: string[]) => void
}) {
  const [input, setInput] = useState('')

  function add() {
    const t = input.trim()
    if (t && !tags.includes(t)) {
      onChange([...tags, t])
      setInput('')
    }
  }

  return (
    <div className="flex flex-col gap-1.5">
      <label className="text-xs font-medium text-muted-foreground">{label}</label>
      <div className="flex flex-wrap items-center gap-1.5">
        {tags.map((tag, i) => (
          <span
            key={`${tag}-${i}`}
            className="flex items-center gap-0.5 rounded-md bg-accent px-2 py-0.5 text-xs font-medium text-foreground"
          >
            {tag}
            <button
              type="button"
              onClick={() => onChange(tags.filter((_, idx) => idx !== i))}
              className="ml-0.5 text-muted-foreground hover:text-destructive text-[10px]"
            >
              x
            </button>
          </span>
        ))}
        <div className="flex items-center gap-1">
          <input
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); add() } }}
            placeholder="+"
            className="h-6 w-16 rounded border border-input bg-background px-1 text-xs"
          />
        </div>
      </div>
      <p className="text-xs text-muted-foreground/70">{hint}</p>
    </div>
  )
}

export function CustomerSupportConfigForm({ initial, onSubmit, onCancel }: CustomerSupportConfigFormProps) {
  const [state, setState] = useState<FormState>(() => stateFromInitial(initial))

  function update(patch: Partial<FormState>) {
    setState((s) => ({ ...s, ...patch }))
  }

  function handleSave() {
    onSubmit({
      ...groupMatchToRecord(state.groupMatch),
      responseStyle: state.responseStyle,
      greeting: state.greeting,
      closing: state.closing,
      escalationKeywords: state.tags.escalationKeywords,
      techKeywords: state.tags.techKeywords,
      summaryCron: state.summaryCron,
      language: 'zh',
    })
  }

  const valid = state.groupMatch.keywords.length > 0

  return (
    <div className="flex flex-col gap-4">
      <p className="text-xs leading-relaxed text-muted-foreground">
        Configure customer support settings. The employee will scan matching DingTalk groups for business inquiries, search FAQ and past conversations, then draft friendly replies for your review.
      </p>

      <GroupMatchInput
        value={state.groupMatch}
        onChange={(gm) => update({ groupMatch: gm })}
        label="Monitor groups (keyword matching)"
        defaultKeywords={['服务', '客户', '售后']}
        defaultExclude={['内部', '测试']}
      />

      {/* Response style */}
      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">Response style</label>
        <div className="flex items-center gap-3 text-sm">
          {RESPONSE_STYLES.map((opt) => (
            <label key={opt.value} className="flex items-center gap-1.5">
              <input
                type="radio"
                name="responseStyle"
                value={opt.value}
                checked={state.responseStyle === opt.value}
                onChange={() => update({ responseStyle: opt.value })}
              />
              {opt.label}
            </label>
          ))}
        </div>
      </div>

      {/* Greeting / Closing */}
      <div className="flex gap-3">
        <div className="flex flex-1 flex-col gap-1.5">
          <label className="text-xs font-medium text-muted-foreground">Greeting</label>
          <Input
            value={state.greeting}
            onChange={(e) => update({ greeting: e.target.value })}
            placeholder="您好，"
            className="text-xs"
          />
        </div>
        <div className="flex flex-1 flex-col gap-1.5">
          <label className="text-xs font-medium text-muted-foreground">Closing</label>
          <Input
            value={state.closing}
            onChange={(e) => update({ closing: e.target.value })}
            placeholder="如还有其他问题随时告诉我们~"
            className="text-xs"
          />
        </div>
      </div>

      {/* Escalation keywords */}
      <InlineTagEditor
        label="Escalation keywords (skip auto-reply, flag human intervention)"
        hint="Messages containing these words will be flagged for manual handling."
        tags={state.tags.escalationKeywords}
        onChange={(next) => update({ tags: { ...state.tags, escalationKeywords: next } })}
      />

      {/* Tech keywords */}
      <InlineTagEditor
        label="Tech keywords (route to tech support)"
        hint="Messages containing these words will be tagged as technical issues."
        tags={state.tags.techKeywords}
        onChange={(next) => update({ tags: { ...state.tags, techKeywords: next } })}
      />

      {/* Summary cron */}
      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">Conversation summary frequency</label>
        <div className="flex items-center gap-3 text-sm">
          {SUMMARY_OPTIONS.map((opt) => (
            <label key={opt.value} className="flex items-center gap-1.5">
              <input
                type="radio"
                name="summaryCron"
                value={opt.value}
                checked={state.summaryCron === opt.value}
                onChange={() => update({ summaryCron: opt.value })}
              />
              {opt.label}
            </label>
          ))}
        </div>
      </div>

      <div className="flex items-center justify-end gap-2 pt-2">
        <Button variant="ghost" onClick={onCancel}>Cancel</Button>
        <Button onClick={handleSave} disabled={!valid}>Save</Button>
      </div>
    </div>
  )
}
