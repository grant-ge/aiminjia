/**
 * Minimal JSON Schema-driven form renderer for digital-employee instance config.
 *
 * This is NOT a full JSON Schema implementation — it handles the subset
 * we actually use on the backend template `resource_config_schema` field:
 *
 *   - object (top-level only, flat properties)
 *   - string (text input, textarea with `"ui:widget":"textarea"`,
 *             URL with `"format":"uri"`, email with `"format":"email"`)
 *   - number / integer (numeric input)
 *   - boolean (switch)
 *   - enum (single-select dropdown when `enum` is present on a string)
 *   - array of string (tag-style input, comma-separated)
 *   - array of enum (multi-select checkbox group)
 *
 * Intentionally out of scope (can be added later if demand exists):
 *   - nested objects
 *   - arrays of objects (one template today needs this — monitoringUrls —
 *     but it already has a hand-tuned MonitoringUrlsForm)
 *   - oneOf / anyOf / $ref
 *   - conditional (if/then/else) schemas
 *
 * The contract with callers:
 *   - `schema` is a plain JSON object; no mutation
 *   - `value` is the current config value (partial allowed)
 *   - `onSubmit(next)` is called with a validated, normalized value
 *     that always matches the schema's `required` / `type` constraints
 *   - `onCancel()` is called when the user clicks cancel
 *
 * See lotus/docs/superpowers/specs/2026-05-10-employee-templates-as-a-service.md §4
 * for the schema decision that drove this.
 */

import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Plus, X } from 'lucide-react'

import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { Button } from '@/components/ui/button'

// Minimal subset of JSON Schema we care about.
export interface JsonSchemaProperty {
  type?: 'string' | 'number' | 'integer' | 'boolean' | 'array'
  title?: string
  description?: string
  default?: unknown
  enum?: string[]
  format?: 'uri' | 'email' | string
  minLength?: number
  maxLength?: number
  minimum?: number
  maximum?: number
  minItems?: number
  maxItems?: number
  items?: JsonSchemaProperty
  /** UI hint — `"textarea"` to render a string as multi-line. */
  'ui:widget'?: 'textarea' | 'input'
  /** Optional placeholder displayed in the input when empty. */
  'ui:placeholder'?: string
}

export interface JsonSchema {
  type?: 'object'
  properties?: Record<string, JsonSchemaProperty>
  required?: string[]
}

export interface SchemaFormProps {
  schema: JsonSchema
  initial?: Record<string, unknown>
  onSubmit: (next: Record<string, unknown>) => void
  onCancel: () => void
}

type Errors = Record<string, string>

function isValidUrl(s: string): boolean {
  try {
    const u = new URL(s)
    return u.protocol === 'http:' || u.protocol === 'https:'
  } catch {
    return false
  }
}

function isValidEmail(s: string): boolean {
  // Intentionally simple — matches what the employee config forms historically accept.
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(s)
}

/**
 * Validate a single value against a schema property. Returns an error
 * string or `null` on success. Used both for live feedback and the final
 * submit check.
 */
function validateValue(
  key: string,
  prop: JsonSchemaProperty,
  value: unknown,
  required: boolean,
): string | null {
  const empty =
    value === undefined ||
    value === null ||
    value === '' ||
    (Array.isArray(value) && value.length === 0)
  if (empty) {
    return required ? `${prop.title ?? key} 是必填项` : null
  }

  switch (prop.type) {
    case 'string': {
      const s = String(value)
      if (prop.enum && !prop.enum.includes(s)) {
        return `${prop.title ?? key} 必须是允许的值之一`
      }
      if (prop.minLength !== undefined && s.length < prop.minLength) {
        return `${prop.title ?? key} 至少 ${prop.minLength} 个字符`
      }
      if (prop.maxLength !== undefined && s.length > prop.maxLength) {
        return `${prop.title ?? key} 最多 ${prop.maxLength} 个字符`
      }
      if (prop.format === 'uri' && !isValidUrl(s)) {
        return `${prop.title ?? key} 不是合法的 URL`
      }
      if (prop.format === 'email' && !isValidEmail(s)) {
        return `${prop.title ?? key} 不是合法的邮箱`
      }
      return null
    }
    case 'number':
    case 'integer': {
      const n = typeof value === 'number' ? value : Number(value)
      if (Number.isNaN(n)) {
        return `${prop.title ?? key} 必须是数字`
      }
      if (prop.type === 'integer' && !Number.isInteger(n)) {
        return `${prop.title ?? key} 必须是整数`
      }
      if (prop.minimum !== undefined && n < prop.minimum) {
        return `${prop.title ?? key} 不能小于 ${prop.minimum}`
      }
      if (prop.maximum !== undefined && n > prop.maximum) {
        return `${prop.title ?? key} 不能大于 ${prop.maximum}`
      }
      return null
    }
    case 'boolean':
      return null
    case 'array': {
      if (!Array.isArray(value)) {
        return `${prop.title ?? key} 必须是数组`
      }
      if (prop.minItems !== undefined && value.length < prop.minItems) {
        return `${prop.title ?? key} 至少 ${prop.minItems} 项`
      }
      if (prop.maxItems !== undefined && value.length > prop.maxItems) {
        return `${prop.title ?? key} 最多 ${prop.maxItems} 项`
      }
      // Per-item validation for arrays of strings with URL/email format.
      if (prop.items?.type === 'string') {
        for (let i = 0; i < value.length; i++) {
          const itemErr = validateValue(`${key}[${i}]`, prop.items, value[i], true)
          if (itemErr) return itemErr
        }
      }
      return null
    }
    default:
      return null
  }
}

/** Seed form state from `initial`, falling back to per-property `default`. */
function buildInitialState(
  schema: JsonSchema,
  initial: Record<string, unknown> | undefined,
): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  const props = schema.properties ?? {}
  for (const [key, prop] of Object.entries(props)) {
    if (initial && key in initial) {
      out[key] = initial[key]
    } else if (prop.default !== undefined) {
      out[key] = prop.default
    } else if (prop.type === 'boolean') {
      out[key] = false
    } else if (prop.type === 'array') {
      out[key] = []
    } else {
      out[key] = ''
    }
  }
  return out
}

export function SchemaForm({ schema, initial, onSubmit, onCancel }: SchemaFormProps) {
  const { t } = useTranslation()
  const [value, setValue] = useState<Record<string, unknown>>(() =>
    buildInitialState(schema, initial),
  )
  const [touched, setTouched] = useState<Record<string, boolean>>({})

  const required = useMemo(() => new Set(schema.required ?? []), [schema])
  const props = schema.properties ?? {}

  const errors: Errors = useMemo(() => {
    const errs: Errors = {}
    for (const [key, prop] of Object.entries(props)) {
      const err = validateValue(key, prop, value[key], required.has(key))
      if (err) errs[key] = err
    }
    return errs
  }, [props, required, value])

  const hasErrors = Object.keys(errors).length > 0

  function updateField(key: string, v: unknown) {
    setValue((prev) => ({ ...prev, [key]: v }))
    setTouched((prev) => ({ ...prev, [key]: true }))
  }

  function handleSubmit() {
    // Mark all fields touched so errors render for the user.
    const allTouched: Record<string, boolean> = {}
    for (const key of Object.keys(props)) allTouched[key] = true
    setTouched(allTouched)
    if (hasErrors) return
    // Normalize: coerce numeric strings to numbers before submitting.
    const out: Record<string, unknown> = {}
    for (const [key, prop] of Object.entries(props)) {
      const v = value[key]
      if (v === '' && !required.has(key)) continue
      if ((prop.type === 'number' || prop.type === 'integer') && typeof v === 'string') {
        out[key] = Number(v)
      } else {
        out[key] = v
      }
    }
    onSubmit(out)
  }

  return (
    <div data-aijia-resource-form="schema" className="flex flex-col gap-4">
      {Object.entries(props).map(([key, prop]) => (
        <FieldRow
          key={key}
          name={key}
          prop={prop}
          value={value[key]}
          required={required.has(key)}
          error={touched[key] ? errors[key] : undefined}
          onChange={(v) => updateField(key, v)}
        />
      ))}

      <div className="flex items-center justify-end gap-2 pt-2">
        <Button variant="ghost" data-aijia-resource-action="cancel" onClick={onCancel}>
          {t('employee.config.cancel', 'Cancel')}
        </Button>
        <Button data-aijia-resource-action="save" onClick={handleSubmit}>
          {t('employee.config.save', 'Save')}
        </Button>
      </div>
    </div>
  )
}

interface FieldRowProps {
  name: string
  prop: JsonSchemaProperty
  value: unknown
  required: boolean
  error?: string
  onChange: (v: unknown) => void
}

function FieldRow({ name, prop, value, required, error, onChange }: FieldRowProps) {
  const label = prop.title ?? name
  const placeholder = prop['ui:placeholder']

  return (
    <div data-aijia-resource-field={name} className="flex flex-col gap-1.5">
      <label className="text-xs font-medium text-muted-foreground">
        {label}
        {required && <span className="ml-0.5 text-destructive">*</span>}
      </label>
      {prop.description && (
        <p className="text-xs text-muted-foreground/80">{prop.description}</p>
      )}
      <Widget prop={prop} value={value} onChange={onChange} placeholder={placeholder} />
      {error && <p className="text-xs text-destructive">{error}</p>}
    </div>
  )
}

interface WidgetProps {
  prop: JsonSchemaProperty
  value: unknown
  onChange: (v: unknown) => void
  placeholder?: string
}

function Widget({ prop, value, onChange, placeholder }: WidgetProps) {
  // Boolean → native checkbox (no Switch dependency assumption).
  if (prop.type === 'boolean') {
    return (
      <label className="flex items-center gap-2">
        <input
          type="checkbox"
          checked={!!value}
          onChange={(e) => onChange(e.target.checked)}
          className="h-4 w-4 rounded-md border-border"
        />
        <span className="text-sm text-foreground">
          {value ? '开启' : '关闭'}
        </span>
      </label>
    )
  }

  // Enum string → select.
  if (prop.type === 'string' && prop.enum) {
    return (
      <select
        value={(value as string) ?? ''}
        onChange={(e) => onChange(e.target.value)}
        className="h-9 rounded-md border border-input bg-background px-3 text-sm"
      >
        <option value="" disabled>
          请选择
        </option>
        {prop.enum.map((opt) => (
          <option key={opt} value={opt}>
            {opt}
          </option>
        ))}
      </select>
    )
  }

  // Number / integer → numeric input.
  if (prop.type === 'number' || prop.type === 'integer') {
    return (
      <Input
        type="number"
        step={prop.type === 'integer' ? 1 : 'any'}
        value={value === undefined || value === null ? '' : String(value)}
        min={prop.minimum}
        max={prop.maximum}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
      />
    )
  }

  // Array of string / array of enum.
  if (prop.type === 'array') {
    return (
      <ArrayWidget
        prop={prop}
        value={Array.isArray(value) ? value : []}
        onChange={onChange}
        placeholder={placeholder}
      />
    )
  }

  // Default: string — textarea if `ui:widget`='textarea', else single-line.
  if (prop['ui:widget'] === 'textarea') {
    return (
      <Textarea
        value={(value as string) ?? ''}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
        rows={4}
      />
    )
  }
  return (
    <Input
      type={prop.format === 'email' ? 'email' : 'text'}
      value={(value as string) ?? ''}
      placeholder={placeholder}
      onChange={(e) => onChange(e.target.value)}
    />
  )
}

interface ArrayWidgetProps {
  prop: JsonSchemaProperty
  value: unknown[]
  onChange: (v: unknown[]) => void
  placeholder?: string
}

function ArrayWidget({ prop, value, onChange, placeholder }: ArrayWidgetProps) {
  const items = prop.items
  const [draft, setDraft] = useState('')

  // Array of enum → checkbox group (multi-select).
  if (items?.type === 'string' && items.enum) {
    const selected = new Set(value as string[])
    return (
      <div className="flex flex-wrap gap-3">
        {items.enum.map((opt) => (
          <label key={opt} className="flex items-center gap-1.5 text-sm">
            <input
              type="checkbox"
              checked={selected.has(opt)}
              onChange={(e) => {
                const next = new Set(selected)
                if (e.target.checked) next.add(opt)
                else next.delete(opt)
                onChange(Array.from(next))
              }}
              className="h-4 w-4 rounded-md border-border"
            />
            <span>{opt}</span>
          </label>
        ))}
      </div>
    )
  }

  // Array of string → tag-style input. Enter or comma to commit.
  function commitDraft() {
    const t = draft.trim()
    if (!t) return
    if (!value.includes(t)) {
      onChange([...value, t])
    }
    setDraft('')
  }

  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap gap-1.5">
        {(value as string[]).map((tag, i) => (
          <span
            key={`${tag}-${i}`}
            className="inline-flex items-center gap-1 rounded-md bg-muted px-2 py-0.5 text-xs"
          >
            {tag}
            <Button unstyled
              type="button"
              onClick={() => onChange(value.filter((_, idx) => idx !== i))}
              className="text-muted-foreground hover:text-destructive"
            >
              <X className="h-3 w-3" />
            </Button>
          </span>
        ))}
      </div>
      <div className="flex items-center gap-2">
        <Input
          value={draft}
          placeholder={placeholder}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ',') {
              e.preventDefault()
              commitDraft()
            }
          }}
          onBlur={commitDraft}
        />
        <Button unstyled
          type="button"
          onClick={commitDraft}
          className="flex items-center gap-1 rounded-md border border-border px-2 py-1 text-xs text-muted-foreground hover:text-foreground"
        >
          <Plus className="h-3 w-3" />
        </Button>
      </div>
    </div>
  )
}
