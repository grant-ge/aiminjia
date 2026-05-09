import { useState } from 'react'

import { Button } from '@/components/ui/button'
import { GroupMatchInput, groupMatchFromRecord, groupMatchToRecord, type GroupMatchConfig } from './GroupMatchInput'

interface TechSupportConfigFormProps {
  initial: Record<string, unknown>
  onSubmit: (next: Record<string, unknown>) => void
  onCancel: () => void
}

type ResponseStyle = 'professional' | 'friendly' | 'concise'
type SummaryCron = 'daily' | 'weekly' | 'off'

interface FormState {
  groupMatch: GroupMatchConfig
  responseStyle: ResponseStyle
  summaryCron: SummaryCron
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

function stateFromInitial(initial: Record<string, unknown>): FormState {
  const gm = groupMatchFromRecord(initial)
  // If no keywords set, use defaults
  if (gm.keywords.length === 0) {
    gm.keywords = ['技术', '对接', '集成']
    gm.exclude = ['内部', '测试']
  }
  const responseStyle = (['professional', 'friendly', 'concise'].includes(initial.responseStyle as string)
    ? initial.responseStyle
    : 'professional') as ResponseStyle
  const summaryCron = (['daily', 'weekly', 'off'].includes(initial.summaryCron as string)
    ? initial.summaryCron
    : 'weekly') as SummaryCron
  return { groupMatch: gm, responseStyle, summaryCron }
}

export function TechSupportConfigForm({ initial, onSubmit, onCancel }: TechSupportConfigFormProps) {
  const [state, setState] = useState<FormState>(() => stateFromInitial(initial))

  function update(patch: Partial<FormState>) {
    setState((s) => ({ ...s, ...patch }))
  }

  function handleSave() {
    onSubmit({
      ...groupMatchToRecord(state.groupMatch),
      responseStyle: state.responseStyle,
      summaryCron: state.summaryCron,
      language: 'zh',
      autoSend: false,
    })
  }

  const valid = state.groupMatch.keywords.length > 0

  return (
    <div className="flex flex-col gap-4">
      <p className="text-xs leading-relaxed text-muted-foreground">
        Configure tech support settings. The employee will scan matching DingTalk groups for technical questions, search knowledge base and past experience, then draft replies for your review.
      </p>

      <GroupMatchInput
        value={state.groupMatch}
        onChange={(gm) => update({ groupMatch: gm })}
        label="Monitor groups (keyword matching)"
        defaultKeywords={['技术', '对接', '集成']}
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

      {/* Summary cron */}
      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">Experience summary frequency</label>
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
        <p className="text-xs text-muted-foreground/70">
          Periodically summarize accumulated Q&A into a knowledge digest.
        </p>
      </div>

      <div className="flex items-center justify-end gap-2 pt-2">
        <Button variant="ghost" onClick={onCancel}>Cancel</Button>
        <Button onClick={handleSave} disabled={!valid}>Save</Button>
      </div>
    </div>
  )
}
