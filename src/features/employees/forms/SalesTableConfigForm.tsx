import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronDown, ChevronRight } from 'lucide-react'

import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'

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
 * Returns null only if baseId can't be located. Missing sheetId yields tableId === ''
 * so the user can save anyway and let the employee resolve the sheet via dialog.
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
    const inner = new URLSearchParams(iframeQuery)
    tableId = inner.get('sheetId') ?? ''
  } catch {
    /* fall through */
  }
  if (!baseId) return null
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

type FieldMappingError =
  | { code: 'object' }
  | { code: 'string'; field: string }
  | { code: 'parse'; error: string }

function tryParseFieldMapping(
  raw: string,
): { ok: true; value: Record<string, string> } | { ok: false; err: FieldMappingError } {
  const trimmed = raw.trim()
  if (!trimmed) return { ok: true, value: {} }
  try {
    const parsed = JSON.parse(trimmed) as unknown
    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
      return { ok: false, err: { code: 'object' } }
    }
    const out: Record<string, string> = {}
    for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof v !== 'string') {
        return { ok: false, err: { code: 'string', field: k } }
      }
      out[k] = v
    }
    return { ok: true, value: out }
  } catch (err) {
    return { ok: false, err: { code: 'parse', error: String(err) } }
  }
}

export function SalesTableConfigForm({ initial, onSubmit, onCancel }: SalesTableConfigFormProps) {
  const { t } = useTranslation()
  const [state, setState] = useState<FormState>(() => stateFromInitial(initial))

  const parsed = useMemo(() => parseDingtalkAitableUrl(state.shareUrl), [state.shareUrl])
  // Effective ids: parsed wins; fall back to manual override if parse failed.
  const effectiveBaseId = parsed?.baseId || state.baseId.trim()
  const effectiveTableId = parsed?.tableId || state.tableId.trim()

  const fieldMappingResult = useMemo(
    () => tryParseFieldMapping(state.fieldMappingRaw),
    [state.fieldMappingRaw],
  )

  const valid = fieldMappingResult.ok

  function update(patch: Partial<FormState>) {
    setState((s) => ({ ...s, ...patch }))
  }

  function handleSave() {
    if (!fieldMappingResult.ok) return
    onSubmit({
      shareUrl: state.shareUrl.trim() || undefined,
      baseId: effectiveBaseId || undefined,
      tableId: effectiveTableId || undefined,
      fieldMapping: fieldMappingResult.value,
      scope: state.scope,
    })
  }

  function fieldMappingErrorMsg(): string {
    if (fieldMappingResult.ok) return ''
    const { err } = fieldMappingResult
    if (err.code === 'object') return t('employee.config.salesTable.fieldMappingError_object')
    if (err.code === 'string') return t('employee.config.salesTable.fieldMappingError_string', { field: err.field })
    return t('employee.config.salesTable.fieldMappingError_parse', { error: err.error })
  }

  const showParseHint = state.shareUrl.trim().length > 0

  return (
    <div data-aijia-resource-form="sales-table" className="flex flex-col gap-4">
      <p className="text-xs leading-relaxed text-muted-foreground">
        {t('employee.config.salesTable.intro')}
      </p>

      {/* Share URL — primary input */}
      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">
          {t('employee.config.salesTable.shareUrlLabel')}
        </label>
        <Input
          value={state.shareUrl}
          onChange={(e) => update({ shareUrl: e.target.value })}
          data-aijia-resource-field="shareUrl"
          placeholder={t('employee.config.salesTable.shareUrlPlaceholder')}
          className="font-mono text-xs"
        />
        {showParseHint && parsed && (
          <p className="text-xs text-emerald-600">
            {t('employee.config.salesTable.parseOk', {
              baseId: `${parsed.baseId.slice(0, 12)}…`,
              tableSuffix: parsed.tableId
                ? ` / table ${parsed.tableId}`
                : t('employee.config.salesTable.parseOkNoSheet'),
            })}
          </p>
        )}
        {showParseHint && !parsed && (
          <p className="text-xs text-amber-600">
            {t('employee.config.salesTable.parseError')}
          </p>
        )}
      </div>

      {/* Manual override — collapsed by default unless parse failed */}
      {showParseHint && !parsed && (
        <div className="flex flex-col gap-3 rounded-md border border-[rgba(var(--border-rgb),0.60)] bg-[rgba(var(--accent-rgb),0.20)] p-3">
          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-medium text-muted-foreground">
              {t('employee.config.salesTable.baseIdLabel')}
            </label>
            <Input
              value={state.baseId}
              onChange={(e) => update({ baseId: e.target.value })}
              data-aijia-resource-field="baseId"
              placeholder={t('employee.config.salesTable.baseIdPlaceholder')}
              className="font-mono text-xs"
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-medium text-muted-foreground">
              {t('employee.config.salesTable.tableIdLabel')}
            </label>
            <Input
              value={state.tableId}
              onChange={(e) => update({ tableId: e.target.value })}
              data-aijia-resource-field="tableId"
              placeholder={t('employee.config.salesTable.tableIdPlaceholder')}
              className="font-mono text-xs"
            />
          </div>
        </div>
      )}

      {/* Scope */}
      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">
          {t('employee.config.salesTable.scopeLabel')}
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
            {t('employee.config.salesTable.scopeSelf')}
          </label>
          <label className="flex items-center gap-1.5">
            <input
              type="radio"
              name="scope"
              value="department"
              checked={state.scope === 'department'}
              onChange={() => update({ scope: 'department' })}
            />
            {t('employee.config.salesTable.scopeDepartment')}
          </label>
        </div>
      </div>

      {/* Advanced — field mapping JSON */}
      <div className="flex flex-col gap-1.5">
        <Button unstyled
          type="button"
          onClick={() => update({ showAdvanced: !state.showAdvanced })}
          className="flex items-center gap-1 self-start text-xs text-muted-foreground hover:text-foreground"
        >
          {state.showAdvanced ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
          {t('employee.config.salesTable.advancedToggle')}
        </Button>
        {state.showAdvanced && (
          <>
            <textarea
              value={state.fieldMappingRaw}
              onChange={(e) => update({ fieldMappingRaw: e.target.value })}
              data-aijia-resource-field="fieldMapping"
              rows={9}
              placeholder={DEFAULT_FIELD_MAPPING_TEMPLATE}
              className="rounded-md border border-input bg-background px-3 py-2 font-mono text-xs leading-relaxed"
            />
            <p className="text-xs text-[rgba(var(--muted-foreground-rgb),0.70)]">
              {t('employee.config.salesTable.fieldMappingHint')}
            </p>
            {!fieldMappingResult.ok && (
              <p className="text-xs text-destructive">{fieldMappingErrorMsg()}</p>
            )}
          </>
        )}
      </div>

      <div className="flex items-center justify-end gap-2 pt-2">
        <Button variant="ghost" data-aijia-resource-action="cancel" onClick={onCancel}>{t('employee.config.cancel')}</Button>
        <Button data-aijia-resource-action="save" onClick={handleSave} disabled={!valid}>{t('employee.config.save')}</Button>
      </div>
    </div>
  )
}
