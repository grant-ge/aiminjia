import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'

interface WeeklyReportConfigFormProps {
  initial: Record<string, unknown>
  onSubmit: (next: Record<string, unknown>) => void
  onCancel: () => void
}

type ReportTemplate = 'standard' | 'brief' | 'okr'
type ReportScope = 'self' | 'team'

interface FormState {
  template: ReportTemplate
  watchGroupsInput: string
  scope: ReportScope
  language: 'zh' | 'en'
}

function parseGroups(input: string): string[] {
  return input
    .split(/[,，;；\n]/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0)
}

function stateFromInitial(initial: Record<string, unknown>): FormState {
  const template = (['standard', 'brief', 'okr'].includes(initial.template as string)
    ? initial.template
    : 'standard') as ReportTemplate
  const groups = Array.isArray(initial.watchGroups)
    ? (initial.watchGroups as unknown[]).filter((g): g is string => typeof g === 'string')
    : []
  const scope = initial.scope === 'team' ? 'team' : 'self'
  const language = initial.language === 'en' ? 'en' : 'zh'

  return {
    template,
    watchGroupsInput: groups.join('，'),
    scope,
    language,
  }
}

export function WeeklyReportConfigForm({ initial, onSubmit, onCancel }: WeeklyReportConfigFormProps) {
  const { t } = useTranslation()
  const [state, setState] = useState<FormState>(() => stateFromInitial(initial))

  const templateOptions = [
    { value: 'standard' as ReportTemplate, label: t('employee.config.weeklyReport.templateStandard'), desc: t('employee.config.weeklyReport.templateStandardDesc') },
    { value: 'brief' as ReportTemplate, label: t('employee.config.weeklyReport.templateBrief'), desc: t('employee.config.weeklyReport.templateBriefDesc') },
    { value: 'okr' as ReportTemplate, label: t('employee.config.weeklyReport.templateOkr'), desc: t('employee.config.weeklyReport.templateOkrDesc') },
  ]

  function update(patch: Partial<FormState>) {
    setState((s) => ({ ...s, ...patch }))
  }

  function handleSave() {
    onSubmit({
      template: state.template,
      watchGroups: parseGroups(state.watchGroupsInput),
      scope: state.scope,
      language: state.language,
    })
  }

  const parsedGroups = parseGroups(state.watchGroupsInput)

  return (
    <div data-aijia-resource-form="weekly-report" className="flex flex-col gap-4">
      <p className="text-xs leading-relaxed text-muted-foreground">
        {t('employee.config.weeklyReport.intro')}
      </p>

      {/* Template style */}
      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">
          {t('employee.config.weeklyReport.styleLabel')}
        </label>
        <div className="flex flex-col gap-2">
          {templateOptions.map((opt) => (
            <label key={opt.value} className="flex items-start gap-2 text-sm">
              <input
                type="radio"
                name="template"
                value={opt.value}
                checked={state.template === opt.value}
                onChange={() => update({ template: opt.value })}
                className="mt-0.5"
              />
              <span>
                <span className="font-medium">{opt.label}</span>
                <span className="text-muted-foreground">（{opt.desc}）</span>
              </span>
            </label>
          ))}
        </div>
      </div>

      {/* Scope */}
      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">
          {t('employee.config.weeklyReport.scopeLabel')}
        </label>
        <div className="flex items-center gap-3 text-sm">
          <label className="flex items-center gap-1.5">
            <input
              type="radio"
              name="scope"
              value="self"
              checked={state.scope === 'self'}
              onChange={() => update({ scope: 'self' })}
            />
            {t('employee.config.weeklyReport.scopeSelf')}
          </label>
          <label className="flex items-center gap-1.5">
            <input
              type="radio"
              name="scope"
              value="team"
              checked={state.scope === 'team'}
              onChange={() => update({ scope: 'team' })}
            />
            {t('employee.config.weeklyReport.scopeTeam')}
          </label>
        </div>
      </div>

      {/* Watch groups — comma-separated text input */}
      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">
          {t('employee.config.weeklyReport.watchGroupsLabel')}
        </label>
        <Input
          value={state.watchGroupsInput}
          onChange={(e) => update({ watchGroupsInput: e.target.value })}
          data-aijia-resource-field="watchGroups"
          placeholder={t('employee.config.weeklyReport.watchGroupsPlaceholder')}
          className="text-xs"
        />
        <p className="text-xs text-muted-foreground/70">
          {t('employee.config.weeklyReport.watchGroupsHintSimple')}
        </p>
        {parsedGroups.length > 0 && (
          <p className="text-xs text-muted-foreground">
            {t('employee.config.weeklyReport.watchGroupsSummary', { count: parsedGroups.length })}
            <span className="ml-1 text-foreground">{parsedGroups.join('、')}</span>
          </p>
        )}
      </div>

      <div className="flex items-center justify-end gap-2 pt-2">
        <Button variant="ghost" data-aijia-resource-action="cancel" onClick={onCancel}>{t('employee.config.cancel')}</Button>
        <Button data-aijia-resource-action="save" onClick={handleSave}>{t('employee.config.save')}</Button>
      </div>
    </div>
  )
}
