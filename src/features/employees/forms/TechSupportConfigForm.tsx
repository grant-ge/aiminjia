import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { GroupMatchInput, groupMatchFromRecord, groupMatchToRecord, type GroupMatchConfig } from './GroupMatchInput'
import { KnowledgeSourcesField, parseKnowledgeSources, type KnowledgeSource } from './KnowledgeSourcesField'
import { Button } from '@/components/ui/button'

interface Props { initial: Record<string, unknown>; onSubmit: (n: Record<string, unknown>) => void; onCancel: () => void }
type ResponseStyle = 'professional' | 'friendly' | 'concise'
type SummaryCron = 'daily' | 'weekly' | 'off'

interface FormState {
  groupMatch: GroupMatchConfig
  responseStyle: ResponseStyle
  summaryCron: SummaryCron
  knowledgeSources: KnowledgeSource[]
}

function stateFromInitial(initial: Record<string, unknown>): FormState {
  const gm = groupMatchFromRecord(initial)
  if (gm.keywords.length === 0) { gm.keywords = ['技术', '对接', '集成']; gm.exclude = ['内部', '测试'] }
  const responseStyle = (['professional', 'friendly', 'concise'].includes(initial.responseStyle as string)
    ? initial.responseStyle : 'professional') as ResponseStyle
  const summaryCron = (['daily', 'weekly', 'off'].includes(initial.summaryCron as string)
    ? initial.summaryCron : 'weekly') as SummaryCron
  return { groupMatch: gm, responseStyle, summaryCron, knowledgeSources: parseKnowledgeSources(initial.knowledgeSources) }
}

export function TechSupportConfigForm({ initial, onSubmit, onCancel }: Props) {
  const { t } = useTranslation()
  const [state, setState] = useState<FormState>(() => stateFromInitial(initial))
  const update = (p: Partial<FormState>) => setState((s) => ({ ...s, ...p }))

  function handleSave() {
    onSubmit({
      ...groupMatchToRecord(state.groupMatch),
      responseStyle: state.responseStyle,
      summaryCron: state.summaryCron,
      knowledgeSources: state.knowledgeSources,
      language: 'zh',
      autoSend: false,
    })
  }

  const styles: ResponseStyle[] = ['professional', 'friendly', 'concise']
  const summaries: SummaryCron[] = ['daily', 'weekly', 'off']

  return (
    <div data-aijia-resource-form="tech-support" className="flex flex-col gap-4">
      <p className="text-xs leading-relaxed text-muted-foreground">{t('employee.config.techSupport.intro')}</p>
      <GroupMatchInput value={state.groupMatch} onChange={(gm) => update({ groupMatch: gm })}
        defaultKeywords={['技术', '对接', '集成']} defaultExclude={['内部', '测试']} />
      <KnowledgeSourcesField value={state.knowledgeSources} onChange={(next) => update({ knowledgeSources: next })} />
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
        <Button variant="ghost" data-aijia-resource-action="cancel" onClick={onCancel}>{t('employee.config.cancel')}</Button>
        <Button data-aijia-resource-action="save" onClick={handleSave}>{t('employee.config.save')}</Button>
      </div>
    </div>
  )
}
