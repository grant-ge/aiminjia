import { useMemo, useState } from 'react'
import { ChevronDown, ChevronRight } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

interface SalesTableConfigFormProps {
  initial: Record<string, unknown>
  onSubmit: (next: Record<string, unknown>) => void
  onCancel: () => void
}

interface FormState {
  /** Pasted DingTalk AI Table URL — single source for baseId/tableId. */
  shareUrl: string
  /** Manually overridden baseId; usually parsed from shareUrl. */
  baseId: string
  /** Manually overridden tableId; usually parsed from shareUrl. */
  tableId: string
  fieldMappingRaw: string
  scope: 'self' | 'department'
  showAdvanced: boolean
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

/**
 * Parse a DingTalk AI Table share URL like:
 *   https://docs.dingtalk.com/i/nodes/<baseId>?iframeQuery=entrance%3Ddata%26sheetId%3D<sheetId>...
 * Returns null if either id can't be located.
 */
function parseDingtalkAitableUrl(input: string): { baseId: string; tableId: string } | null {
  const trimmed = input.trim()
  if (!trimmed) return null
  let u: URL
  try {
    u = new URL(trimmed)
  } catch {
    return null
  }
  const pathMatch = u.pathname.match(/\/nodes\/([A-Za-z0-9_-]+)/)
  const baseId = pathMatch?.[1] ?? ''
  const iframeQuery = u.searchParams.get('iframeQuery') ?? ''
  let tableId = ''
  try {
    // iframeQuery is itself a URL-encoded query string
    const inner = new URLSearchParams(iframeQuery)
    tableId = inner.get('sheetId') ?? ''
  } catch {
    /* fall through */
  }
  if (!baseId || !tableId) return null
  return { baseId, tableId }
}

function stateFromInitial(initial: Record<string, unknown>): FormState {
  const baseId = typeof initial.baseId === 'string' ? initial.baseId : ''
  const tableId = typeof initial.tableId === 'string' ? initial.tableId : ''
  const shareUrl = typeof initial.shareUrl === 'string' ? initial.shareUrl : ''
  const scope = initial.scope === 'department' ? 'department' : 'self'
  let fieldMappingRaw = ''
  if (initial.fieldMapping && typeof initial.fieldMapping === 'object') {
    try {
      fieldMappingRaw = JSON.stringify(initial.fieldMapping, null, 2)
    } catch {
      /* ignore */
    }
  }
  return {
    shareUrl,
    baseId,
    tableId,
    fieldMappingRaw,
    scope,
    showAdvanced: !!fieldMappingRaw,
  }
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

  const parsed = useMemo(() => parseDingtalkAitableUrl(state.shareUrl), [state.shareUrl])
  // Effective ids: parsed wins; fall back to manual override if parse failed.
  const effectiveBaseId = parsed?.baseId || state.baseId.trim()
  const effectiveTableId = parsed?.tableId || state.tableId.trim()

  const fieldMappingResult = useMemo(
    () => tryParseFieldMapping(state.fieldMappingRaw),
    [state.fieldMappingRaw],
  )

  const valid =
    effectiveBaseId.length > 0
    && effectiveTableId.length > 0
    && fieldMappingResult.ok

  function update(patch: Partial<FormState>) {
    setState((s) => ({ ...s, ...patch }))
  }

  function handleSave() {
    if (!fieldMappingResult.ok || !valid) return
    onSubmit({
      shareUrl: state.shareUrl.trim() || undefined,
      baseId: effectiveBaseId,
      tableId: effectiveTableId,
      fieldMapping: fieldMappingResult.value,
      scope: state.scope,
    })
  }

  const showParseHint = state.shareUrl.trim().length > 0

  return (
    <div className="flex flex-col gap-4">
      <p className="text-xs leading-relaxed text-muted-foreground">
        粘贴钉钉 AI 表格的链接即可。小销会基于这张表读取在谈客户。字段映射可以留空——首次派活时员工会通过对话引导你完成。
      </p>

      {/* Share URL — primary input */}
      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">钉钉 AI 表格链接</label>
        <Input
          value={state.shareUrl}
          onChange={(e) => update({ shareUrl: e.target.value })}
          placeholder="https://docs.dingtalk.com/i/nodes/.../?iframeQuery=..."
          className="font-mono text-xs"
        />
        {showParseHint && parsed && (
          <p className="text-xs text-emerald-600">
            ✓ 已识别：base <span className="font-mono">{parsed.baseId.slice(0, 12)}…</span> /
            table <span className="font-mono">{parsed.tableId}</span>
          </p>
        )}
        {showParseHint && !parsed && (
          <p className="text-xs text-amber-600">
            链接格式无法识别。可以下方手动填 baseId / tableId，或直接保存——派活时小销会引导你定位表格。
          </p>
        )}
      </div>

      {/* Manual override — collapsed by default unless parse failed */}
      {showParseHint && !parsed && (
        <div className="flex flex-col gap-3 rounded-md border border-border/60 bg-accent/20 p-3">
          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-medium text-muted-foreground">Base ID（手动填写）</label>
            <Input
              value={state.baseId}
              onChange={(e) => update({ baseId: e.target.value })}
              placeholder="例如 oP0MALyR8k73krjmIQeMLXrz83bzYmDO"
              className="font-mono text-xs"
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-medium text-muted-foreground">Table ID（手动填写）</label>
            <Input
              value={state.tableId}
              onChange={(e) => update({ tableId: e.target.value })}
              placeholder="例如 26qlT0c"
              className="font-mono text-xs"
            />
          </div>
        </div>
      )}

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

      {/* Advanced — field mapping JSON */}
      <div className="flex flex-col gap-1.5">
        <button
          type="button"
          onClick={() => update({ showAdvanced: !state.showAdvanced })}
          className="flex items-center gap-1 self-start text-xs text-muted-foreground hover:text-foreground"
        >
          {state.showAdvanced ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
          高级：预填字段映射
        </button>
        {state.showAdvanced && (
          <>
            <textarea
              value={state.fieldMappingRaw}
              onChange={(e) => update({ fieldMappingRaw: e.target.value })}
              rows={9}
              placeholder={DEFAULT_FIELD_MAPPING_TEMPLATE}
              className="rounded-md border border-input bg-background px-3 py-2 font-mono text-xs leading-relaxed"
            />
            <p className="text-xs text-muted-foreground/70">
              留空也可以——派活时员工会通过 dws 列出真实字段名供你选择。
            </p>
            {!fieldMappingResult.ok && (
              <p className="text-xs text-destructive">{fieldMappingResult.error}</p>
            )}
          </>
        )}
      </div>

      <div className="flex items-center justify-end gap-2 pt-2">
        <Button variant="ghost" onClick={onCancel}>取消</Button>
        <Button onClick={handleSave} disabled={!valid}>保存</Button>
      </div>
    </div>
  )
}
