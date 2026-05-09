import { useState } from 'react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

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

const TEMPLATE_OPTIONS: { value: ReportTemplate; label: string; desc: string }[] = [
  { value: 'standard', label: '标准', desc: '日程 + 待办 + 群聊，分日归类' },
  { value: 'brief', label: '简洁', desc: '要点列表，一页纸' },
  { value: 'okr', label: 'OKR 对齐', desc: '按 O/KR 归类本周进展' },
]

export function WeeklyReportConfigForm({ initial, onSubmit, onCancel }: WeeklyReportConfigFormProps) {
  const [state, setState] = useState<FormState>(() => stateFromInitial(initial))

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
    <div className="flex flex-col gap-4">
      <p className="text-xs leading-relaxed text-muted-foreground">
        配置周报偏好。小周会在每周五自动汇总钉钉日程、待办和群聊，生成结构化周报，呈现在对话中供你查看与编辑。
      </p>

      {/* Template style */}
      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">周报风格</label>
        <div className="flex flex-col gap-2">
          {TEMPLATE_OPTIONS.map((opt) => (
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
        <label className="text-xs font-medium text-muted-foreground">范围</label>
        <div className="flex items-center gap-3 text-sm">
          <label className="flex items-center gap-1.5">
            <input
              type="radio"
              name="scope"
              value="self"
              checked={state.scope === 'self'}
              onChange={() => update({ scope: 'self' })}
            />
            个人周报
          </label>
          <label className="flex items-center gap-1.5">
            <input
              type="radio"
              name="scope"
              value="team"
              checked={state.scope === 'team'}
              onChange={() => update({ scope: 'team' })}
            />
            团队周报（汇总下属）
          </label>
        </div>
      </div>

      {/* Watch groups — comma-separated text input */}
      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">监听群聊（可选）</label>
        <Input
          value={state.watchGroupsInput}
          onChange={(e) => update({ watchGroupsInput: e.target.value })}
          placeholder="例如：产品周会，研发日常，客户支持"
          className="text-xs"
        />
        <p className="text-xs text-muted-foreground/70">
          填写钉钉群名，多个群用 <span className="font-mono">逗号</span>（中英文均可）或换行分隔。小周会按群名搜索并提取本周关键讨论；留空则跳过群聊摘要。
        </p>
        {parsedGroups.length > 0 && (
          <p className="text-xs text-muted-foreground">
            将监听 {parsedGroups.length} 个群：
            <span className="ml-1 text-foreground">{parsedGroups.join('、')}</span>
          </p>
        )}
      </div>

      <div className="flex items-center justify-end gap-2 pt-2">
        <Button variant="ghost" onClick={onCancel}>取消</Button>
        <Button onClick={handleSave}>保存</Button>
      </div>
    </div>
  )
}
