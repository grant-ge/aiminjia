import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/common/Button'
import type { McpServerConfig } from '@/lib/tauri'
import { parseEnvVars } from './mcpServerFormUtils'

interface McpServerFormProps {
  visible: boolean
  onSubmit: (config: McpServerConfig) => Promise<void>
  onCancel: () => void
  submitting: boolean
}

export function McpServerForm({
  visible,
  onSubmit,
  onCancel,
  submitting,
}: McpServerFormProps) {
  if (!visible) {
    return null
  }

  return (
    <McpServerFormContent
      key={submitting ? 'submitting' : 'idle'}
      onSubmit={onSubmit}
      onCancel={onCancel}
      submitting={submitting}
    />
  )
}

interface McpServerFormContentProps {
  onSubmit: (config: McpServerConfig) => Promise<void>
  onCancel: () => void
  submitting: boolean
}

function McpServerFormContent({
  onSubmit,
  onCancel,
  submitting,
}: McpServerFormContentProps) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [transportType, setTransportType] = useState('stdio')
  const [endpoint, setEndpoint] = useState('')
  const [envVarsText, setEnvVarsText] = useState('')

  const nameHasWhitespace = /\s/.test(name)
  const canSubmit = name.trim().length > 0 && endpoint.trim().length > 0 && !nameHasWhitespace

  const endpointDesc = useMemo(() => {
    return transportType === 'stdio'
      ? t('settings.mcp.form.endpointDescStdio')
      : t('settings.mcp.form.endpointDescHttp')
  }, [t, transportType])

  const handleSubmit = async () => {
    if (!canSubmit || submitting) return

    await onSubmit({
      name: name.trim(),
      transportType,
      endpoint: endpoint.trim(),
      envVars: parseEnvVars(envVarsText),
    })
  }

  return (
    <div
      className="mb-4 rounded-md border p-4 border-border"
      style={{
        borderColor: 'var(--color-border)',
        background: 'var(--color-bg-main)',
      }}
    >
      <div className="grid gap-3 md:grid-cols-2">
        <FormField
          label={t('settings.mcp.form.name')}
          description={t('settings.mcp.form.nameDesc')}
        >
          <input
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder={t('settings.mcp.form.namePlaceholder')}
            className="h-9 w-full rounded-md border px-3 text-sm outline-none border-border"
            style={{
              background: 'var(--color-bg-card)',
              borderColor: 'var(--color-border)',
              color: 'var(--color-text-primary)',
            }}
          />
          {nameHasWhitespace ? (
            <p className="mt-1 text-xs" style={{ color: 'var(--color-semantic-red, #ef4444)' }}>
              {t('settings.mcp.form.nameNoSpaces')}
            </p>
          ) : null}
        </FormField>

        <FormField
          label={t('settings.mcp.form.transportType')}
          description=""
        >
          <select
            value={transportType}
            onChange={(event) => setTransportType(event.target.value)}
            className="h-9 w-full rounded-md border px-3 text-sm outline-none border-border"
            style={{
              background: 'var(--color-bg-card)',
              borderColor: 'var(--color-border)',
              color: 'var(--color-text-primary)',
            }}
          >
            <option value="stdio">stdio</option>
            <option value="http">http</option>
            <option value="sse">sse</option>
          </select>
        </FormField>
      </div>

      <FormField
        label={t('settings.mcp.form.endpoint')}
        description={endpointDesc}
        className="mt-3"
      >
        <input
          value={endpoint}
          onChange={(event) => setEndpoint(event.target.value)}
          placeholder={t('settings.mcp.form.endpointPlaceholder')}
          className="h-9 w-full rounded-md border px-3 text-sm outline-none border-border"
          style={{
            background: 'var(--color-bg-card)',
            borderColor: 'var(--color-border)',
            color: 'var(--color-text-primary)',
          }}
        />
      </FormField>

      <FormField
        label={t('settings.mcp.form.envVars')}
        description={t('settings.mcp.form.envVarsDesc')}
        className="mt-3"
      >
        <textarea
          value={envVarsText}
          onChange={(event) => setEnvVarsText(event.target.value)}
          placeholder={t('settings.mcp.form.envVarsPlaceholder')}
          className="min-h-28 w-full rounded-md border px-3 py-2 text-sm outline-none border-border"
          style={{
            background: 'var(--color-bg-card)',
            borderColor: 'var(--color-border)',
            color: 'var(--color-text-primary)',
            resize: 'vertical',
          }}
        />
      </FormField>

      <div className="mt-4 flex items-center justify-end gap-2">
        <Button variant="secondary" size="sm" onClick={onCancel} disabled={submitting}>
          {t('settings.mcp.form.cancel')}
        </Button>
        <Button variant="primary" size="sm" onClick={handleSubmit} disabled={!canSubmit || submitting}>
          {submitting ? t('settings.mcp.form.submitting') : t('settings.mcp.form.submit')}
        </Button>
      </div>
    </div>
  )
}

function FormField({
  label,
  description,
  className = '',
  children,
}: {
  label: string
  description: string
  className?: string
  children: React.ReactNode
}) {
  return (
    <label className={`block ${className}`}>
      <div className="mb-1 text-sm font-medium" style={{ color: 'var(--color-text-primary)' }}>
        {label}
      </div>
      {description ? (
        <p className="mb-2 text-xs" style={{ color: 'var(--color-text-muted)' }}>
          {description}
        </p>
      ) : null}
      {children}
    </label>
  )
}
