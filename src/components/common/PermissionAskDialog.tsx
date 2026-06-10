import { useEffect, useMemo, useState } from 'react'

import { Button } from './Button'
import { Modal } from './Modal'
import type { PendingAsk } from '@/stores/streamingStore'

type PermissionDestination = 'session' | 'workspace' | 'user'

export interface PermissionAskDecision {
  remember: boolean
  destination: PermissionDestination
}

interface PermissionAskDialogProps {
  open: boolean
  ask: PendingAsk | null
  onAllow: (decision: PermissionAskDecision) => void
  onDeny: (decision: PermissionAskDecision) => void
  onCancel: () => void
}

const DESTINATION_OPTIONS: Array<{
  value: PermissionDestination
  label: string
  description: string
}> = [
  {
    value: 'session',
    label: '仅本次',
    description: '这次放行或拒绝后不保留规则。',
  },
  {
    value: 'workspace',
    label: '记住到工作区',
    description: '只对当前工作区后续操作复用这条规则。',
  },
  {
    value: 'user',
    label: '记住到用户级',
    description: '对当前用户后续同类操作都复用这条规则。',
  },
]

function resolveAvailableDestinations(ask: PendingAsk): PermissionDestination[] {
  const rememberOptions: PermissionDestination[] = ask.rememberOptions ?? ['session']
  const options = new Set<PermissionDestination>(rememberOptions)
  options.add('session')
  return DESTINATION_OPTIONS
    .map((option) => option.value)
    .filter((value) => options.has(value))
}

function resolveInitialDestination(
  ask: PendingAsk,
  availableDestinations: PermissionDestination[],
): PermissionDestination {
  if (
    ask.defaultDestination
    && availableDestinations.includes(ask.defaultDestination)
  ) {
    return ask.defaultDestination
  }
  return availableDestinations[0] ?? 'session'
}

function buildDecision(destination: PermissionDestination): PermissionAskDecision {
  return {
    remember: destination !== 'session',
    destination,
  }
}

function buildDenyDecision(): PermissionAskDecision {
  return {
    remember: false,
    destination: 'session',
  }
}

export function PermissionAskDialog({
  open,
  ask,
  onAllow,
  onDeny,
  onCancel,
}: PermissionAskDialogProps) {
  const availableDestinations = useMemo(
    (): PermissionDestination[] => (ask ? resolveAvailableDestinations(ask) : ['session']),
    [ask],
  )
  const [selectionState, setSelectionState] = useState<{
    askKey: string | null
    destination: PermissionDestination
  }>({
    askKey: null,
    destination: 'session',
  })

  useEffect(() => {
    if (!open) return

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        onCancel()
      }
    }

    document.addEventListener('keydown', handleKeyDown)
    return () => document.removeEventListener('keydown', handleKeyDown)
  }, [open, onCancel])

  if (!open || !ask) {
    return null
  }

  const selectedDestination =
    selectionState.askKey === ask.toolCallId
      && availableDestinations.includes(selectionState.destination)
      ? selectionState.destination
      : resolveInitialDestination(ask, availableDestinations)

  const modeLabel = {
    default: '默认模式',
    plan: '计划模式',
    dontAsk: '禁止询问模式',
    acceptEdits: '自动编辑模式',
  }[ask.mode]

  return (
    <Modal
      open={open}
      onClose={onCancel}
      title="工具执行请求"
      size="sm"
      dialogKind="permission-ask"
      dialogTool={ask.toolName}
      footer={(
        <>
          <Button
            variant="secondary"
            onClick={() => onDeny(buildDenyDecision())}
            data-aijia-dialog-action="deny"
          >
            拒绝
          </Button>
          <Button
            variant="primary"
            onClick={() => onAllow(buildDecision(selectedDestination))}
            data-aijia-dialog-action="allow"
          >
            允许
          </Button>
        </>
      )}
    >
      <div className="flex flex-col gap-3">
        <div
          className="text-sm font-semibold"
          style={{ color: 'var(--color-text-primary)' }}
          data-aijia-dialog-title
        >
          {ask.toolName}
        </div>

        <p
          className="text-sm leading-relaxed"
          style={{ color: 'var(--color-text-secondary)' }}
          data-aijia-dialog-description
        >
          {ask.message}
        </p>

        <div
          className="rounded-md border px-3 py-2 text-xs border-border"
          style={{
            background: 'var(--color-bg-subtle)',
            borderColor: 'var(--color-border)',
            color: 'var(--color-text-muted)',
          }}
        >
          当前权限模式：{modeLabel}
        </div>

        {ask.suggestions && ask.suggestions.length > 0 && (
          <div className="flex flex-wrap gap-1.5">
            {ask.suggestions.map((suggestion) => (
              <span
                key={suggestion}
                className="rounded-md px-2 py-0.5 text-xs"
                style={{
                  background: 'var(--color-bg-subtle)',
                  color: 'var(--color-text-muted)',
                  border: '1px solid var(--color-border)',
                }}
              >
                {suggestion}
              </span>
            ))}
          </div>
        )}

        <fieldset className="flex flex-col gap-2">
          <legend
            className="text-sm font-medium"
            style={{ color: 'var(--color-text-primary)' }}
          >
            记住策略
          </legend>
          {DESTINATION_OPTIONS
            .filter((option) => availableDestinations.includes(option.value))
            .map((option) => (
              <label
                key={option.value}
                className="flex cursor-pointer items-start gap-3 rounded-md border px-3 py-2 border-border"
                style={{
                  borderColor:
                    selectedDestination === option.value
                      ? 'var(--color-primary)'
                      : 'var(--color-border)',
                  background:
                    selectedDestination === option.value
                      ? 'var(--color-bg-subtle)'
                      : 'var(--color-bg-card)',
                }}
              >
                <input
                  type="radio"
                  name="permission-destination"
                  aria-label={option.label}
                  value={option.value}
                  checked={selectedDestination === option.value}
                  onChange={() => setSelectionState({
                    askKey: ask.toolCallId,
                    destination: option.value,
                  })}
                />
                <span className="flex flex-col gap-1">
                  <span
                    className="text-sm font-medium"
                    style={{ color: 'var(--color-text-primary)' }}
                  >
                    {option.label}
                  </span>
                  <span
                    className="text-xs"
                    style={{ color: 'var(--color-text-muted)' }}
                  >
                    {option.description}
                  </span>
                </span>
              </label>
            ))}
        </fieldset>
      </div>
    </Modal>
  )
}
