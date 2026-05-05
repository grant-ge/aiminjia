import { useMemo, useState } from 'react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

interface SalesTableConfigFormProps {
  initial: Record<string, unknown>
  onSubmit: (next: Record<string, unknown>) => void
  onCancel: () => void
}

interface FormState {
  baseId: string
  tableId: string
  fieldMappingRaw: string
  scope: 'self' | 'department'
}

const DEFAULT_FIELD_MAPPING_TEMPLATE = `{
  "customerName": "客户名",
  "stage": "阶段",
  "lastContact": "上次联系",
  "nextAction": "下一步动作",
  "nextActionDate": "下一步日期",
  "owner": "负责人",
  "notes": "备注"
}`

function stateFromInitial(initial: Record<string, unknown>): FormState {
  const baseId = typeof initial.baseId === 'string' ? initial.baseId : ''
  const tableId = typeof initial.tableId === 'string' ? initial.tableId : ''
  const scope = initial.scope === 'department' ? 'department' : 'self'
  let fieldMappingRaw = DEFAULT_FIELD_MAPPING_TEMPLATE
  if (initial.fieldMapping && typeof initial.fieldMapping === 'object') {
    try {
      fieldMappingRaw = JSON.stringify(initial.fieldMapping, null, 2)
    } catch {
      // fall back to template
    }
  }
  return { baseId, tableId, fieldMappingRaw, scope }
}

function tryParseFieldMapping(
  raw: string,
): { ok: true; value: Record<string, string> } | { ok: false; error: string } {
  const trimmed = raw.trim()
  if (!trimmed) return { ok: true, value: {} }
  try {
    const parsed = JSON.parse(trimmed) as unknown
    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
      return { ok: false, error: '必须是 JSON 对象（{}），不是数组也不是 null' }
    }
    const out: Record<string, string> = {}
    for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof v !== 'string') {
        return { ok: false, error: `字段 ${k} 的列名必须是字符串` }
      }
      out[k] = v
    }
    return { ok: true, value: out }
  } catch (err) {
    return { ok: false, error: `JSON 解析失败：${String(err)}` }
  }
}

export function SalesTableConfigForm({ initial, onSubmit, onCancel }: SalesTableConfigFormProps) {
  const [state, setState] = useState<FormState>(() => stateFromInitial(initial))

  const fieldMappingResult = useMemo(
    () => tryParseFieldMapping(state.fieldMappingRaw),
    [state.fieldMappingRaw],
  )

  const valid =
    state.baseId.trim().length > 0
    && state.tableId.trim().length > 0
    && fieldMappingResult.ok

  function update(patch: Partial<FormState>) {
    setState((s) => ({ ...s, ...patch }))
  }

  function handleSave() {
    if (!fieldMappingResult.ok || !valid) return
    onSubmit({
      baseId: state.baseId.trim(),
      tableId: state.tableId.trim(),
      fieldMapping: fieldMappingResult.value,
      scope: state.scope,
    })
  }

  return (
    <div className="flex flex-col gap-4">
      <p className="text-xs leading-relaxed text-muted-foreground">
        如果已经知道钉钉 AI 表格的 baseId / tableId，在这里填好可以让小销跳过首次问询；
        留空也可以——派活时小销会通过对话引导你定位表格、确认字段映射，并把结果记到 memory 中。
      </p>

      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">Base ID</label>
        <Input
          value={state.baseId}
          onChange={(e) => update({ baseId: e.target.value })}
          placeholder="dingtalk base id（例如 base_xxxxx）"
          className="font-mono text-xs"
        />
      </div>

      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">Table ID</label>
        <Input
          value={state.tableId}
          onChange={(e) => update({ tableId: e.target.value })}
          placeholder="该 base 内某张表的 id"
          className="font-mono text-xs"
        />
      </div>

      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">字段映射（JSON）</label>
        <textarea
          value={state.fieldMappingRaw}
          onChange={(e) => update({ fieldMappingRaw: e.target.value })}
          rows={9}
          className="rounded-md border border-input bg-background px-3 py-2 font-mono text-xs leading-relaxed"
        />
        <p className="text-xs text-muted-foreground/70">
          把 7 个语义字段（customerName / stage / lastContact / nextAction / nextActionDate / owner / notes）映射到表格中的列名。不需要的字段可删除该键。
        </p>
        {!fieldMappingResult.ok && (
          <p className="text-xs text-destructive">{fieldMappingResult.error}</p>
        )}
      </div>

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
            仅我负责的客户
          </label>
          <label className="flex items-center gap-1.5">
            <input
              type="radio"
              name="scope"
              value="department"
              checked={state.scope === 'department'}
              onChange={() => update({ scope: 'department' })}
            />
            整个部门
          </label>
        </div>
      </div>

      <div className="flex items-center justify-end gap-2 pt-2">
        <Button variant="ghost" onClick={onCancel}>取消</Button>
        <Button onClick={handleSave} disabled={!valid}>保存</Button>
      </div>
    </div>
  )
}
