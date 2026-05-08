import { useState } from 'react'
import { X } from 'lucide-react'

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
  watchGroups: string[]
  newGroup: string
  sendEnabled: boolean
  sendGroupName: string
  scope: ReportScope
  language: 'zh' | 'en'
}

function stateFromInitial(initial: Record<string, unknown>): FormState {
  const template = (['standard', 'brief', 'okr'].includes(initial.template as string)
    ? initial.template
    : 'standard') as ReportTemplate
  const watchGroups = Array.isArray(initial.watchGroups)
    ? (initial.watchGroups as unknown[]).filter((g): g is string => typeof g === 'string')
    : []
  const sendTarget = (initial.sendTarget ?? {}) as Record<string, unknown>
  const scope = initial.scope === 'team' ? 'team' : 'self'
  const language = initial.language === 'en' ? 'en' : 'zh'

  return {
    template,
    watchGroups,
    newGroup: '',
    sendEnabled: !!sendTarget.enabled,
    sendGroupName: typeof sendTarget.groupName === 'string' ? sendTarget.groupName : '',
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

  function addGroup() {
    const name = state.newGroup.trim()
    if (!name || state.watchGroups.includes(name)) return
    update({ watchGroups: [...state.watchGroups, name], newGroup: '' })
  }

  function removeGroup(index: number) {
    update({ watchGroups: state.watchGroups.filter((_, i) => i !== index) })
  }

  function handleSave() {
    onSubmit({
      template: state.template,
      watchGroups: state.watchGroups,
      sendTarget: {
        enabled: state.sendEnabled,
        groupName: state.sendGroupName.trim(),
      },
      scope: state.scope,
      language: state.language,
    })
  }

  return (
    <div className="flex flex-col gap-4">
      <p className="text-xs leading-relaxed text-muted-foreground">
        配置周报偏好。小周会在每周五自动汇总钉钉日程、待办和群聊，生成结构化周报。
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

      {/* Watch groups */}
      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">监听群聊（可选）</label>
        {state.watchGroups.length > 0 && (
          <div className="flex flex-wrap gap-1.5">
            {state.watchGroups.map((g, i) => (
              <span
                key={i}
                className="inline-flex items-center gap-1 rounded-full bg-accent px-2.5 py-1 text-xs"
              >
                {g}
                <button
                  type="button"
                  onClick={() => removeGroup(i)}
                  className="rounded-full p-0.5 hover:bg-muted-foreground/20"
                >
                  <X className="h-3 w-3" />
                </button>
              </span>
            ))}
          </div>
        )}
        <div className="flex items-center gap-2">
          <Input
            value={state.newGroup}
            onChange={(e) => update({ newGroup: e.target.value })}
            placeholder="输入钉钉群名"
            className="text-xs"
            onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); addGroup() } }}
          />
          <Button variant="outline" size="sm" onClick={addGroup} disabled={!state.newGroup.trim()}>
            添加
          </Button>
        </div>
        <p className="text-xs text-muted-foreground/70">
          填写群名，小周会搜索匹配的钉钉群并提取本周关键讨论。留空则跳过群聊摘要。
        </p>
      </div>

      {/* Send target */}
      <div className="flex flex-col gap-1.5">
        <label className="flex items-center gap-2 text-xs font-medium text-muted-foreground">
          <input
            type="checkbox"
            checked={state.sendEnabled}
            onChange={(e) => update({ sendEnabled: e.target.checked })}
          />
          周报生成后发送到钉钉群
          <span className="font-normal text-muted-foreground/70">（每次仍需确认）</span>
        </label>
        {state.sendEnabled && (
          <Input
            value={state.sendGroupName}
            onChange={(e) => update({ sendGroupName: e.target.value })}
            placeholder="发送目标群名"
            className="text-xs"
          />
        )}
      </div>

      <div className="flex items-center justify-end gap-2 pt-2">
        <Button variant="ghost" onClick={onCancel}>取消</Button>
        <Button onClick={handleSave}>确认雇佣</Button>
      </div>
    </div>
  )
}
