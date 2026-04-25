import { useDiagnosticsStore } from '@/stores/diagnosticsStore'
import { recordFrontendDiagnostic } from './tauri'
import type { DiagnosticLevel, FrontendDiagnosticPayload } from './tauri'

export type { DiagnosticLevel }
export type DiagnosticSource = 'frontend' | 'backend'

export interface DiagnosticEvent extends FrontendDiagnosticPayload {
  ts: string
  seq: number
  category: 'diagnostics'
  level: DiagnosticLevel
  source: DiagnosticSource
}

export type DiagnosticInput = Omit<FrontendDiagnosticPayload, 'payload'> & {
  payload?: unknown
}

let seq = 1
const appStartMs = currentMonotonicMs()

function currentMonotonicMs(): number {
  return typeof performance !== 'undefined' ? performance.now() : Date.now()
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return Object.prototype.toString.call(value) === '[object Object]'
}

function isSecretKey(key: string): boolean {
  const lower = key.toLowerCase()
  return (
    lower.includes('token') ||
    lower.includes('apikey') ||
    lower.includes('api_key') ||
    lower.includes('authorization') ||
    lower.includes('cookie') ||
    lower.includes('password') ||
    lower.includes('secret')
  )
}

export function redactDiagnosticPayload<T>(value: T): T {
  if (Array.isArray(value)) {
    return value.map((item) => redactDiagnosticPayload(item)) as T
  }

  if (isPlainObject(value)) {
    return Object.fromEntries(
      Object.entries(value).map(([key, nested]) => [
        key,
        isSecretKey(key) ? '[REDACTED]' : redactDiagnosticPayload(nested),
      ]),
    ) as T
  }

  if (typeof value === 'string') {
    return redactSensitiveText(value) as T
  }

  return value
}

function redactSensitiveText(value: string): string {
  let redacted = redactAfterCaseInsensitive(value, 'bearer ')
  for (const marker of ['access_token=', 'token=', 'api_key=', 'apikey=', 'password=', 'secret=']) {
    redacted = redactAfterCaseInsensitive(redacted, marker)
  }
  return redactPrefixedSecret(redacted, 'sk-')
}

function redactAfterCaseInsensitive(value: string, marker: string): string {
  const lower = value.toLowerCase()
  const markerLower = marker.toLowerCase()
  let output = ''
  let cursor = 0
  let markerStart = lower.indexOf(markerLower, cursor)

  while (markerStart !== -1) {
    const valueStart = markerStart + marker.length
    const valueEnd = findSecretEnd(value, valueStart)
    output += value.slice(cursor, valueStart)
    output += '[REDACTED]'
    cursor = valueEnd
    markerStart = lower.indexOf(markerLower, cursor)
  }

  return output + value.slice(cursor)
}

function redactPrefixedSecret(value: string, prefix: string): string {
  const lower = value.toLowerCase()
  const prefixLower = prefix.toLowerCase()
  let output = ''
  let cursor = 0
  let secretStart = lower.indexOf(prefixLower, cursor)

  while (secretStart !== -1) {
    const secretEnd = findSecretEnd(value, secretStart)
    output += value.slice(cursor, secretStart)
    output += '[REDACTED]'
    cursor = secretEnd
    secretStart = lower.indexOf(prefixLower, cursor)
  }

  return output + value.slice(cursor)
}

function findSecretEnd(value: string, start: number): number {
  for (let index = start; index < value.length; index += 1) {
    if (/\s|["'`,;&)}]/.test(value[index] ?? '')) {
      return index
    }
  }
  return value.length
}

export function summarizePayload(value: unknown): unknown {
  if (typeof value === 'string') {
    if (value.length <= 240) return value
    return `${value.slice(0, 240)}...[truncated ${value.length - 240} chars]`
  }

  if (Array.isArray(value)) {
    const head = value.slice(0, 10).map(summarizePayload)
    if (value.length > 10) head.push(`[truncated ${value.length - 10} items]`)
    return head
  }

  if (isPlainObject(value)) {
    return Object.fromEntries(
      Object.entries(value).map(([key, nested]) => [key, summarizePayload(nested)]),
    )
  }

  return value
}

export function buildDiagnosticEvent(input: DiagnosticInput): DiagnosticEvent {
  const nowMs = currentMonotonicMs()
  const payload = input.payload === undefined
    ? undefined
    : summarizePayload(redactDiagnosticPayload(input.payload))

  return {
    ...input,
    ts: new Date().toISOString(),
    seq: seq++,
    category: 'diagnostics',
    source: 'frontend',
    level: input.level ?? 'info',
    elapsedMs: input.elapsedMs ?? Math.max(0, Math.round(nowMs - appStartMs)),
    payload,
  }
}

export function recordDiagnostic(input: DiagnosticInput): DiagnosticEvent {
  const event = buildDiagnosticEvent(input)
  useDiagnosticsStore.getState().appendDiagnostic(event)
  void recordFrontendDiagnostic(event).catch((error: unknown) => {
    useDiagnosticsStore.getState().appendDiagnostic(
      buildDiagnosticEvent({
        event: 'diagnostics.forward.failed',
        level: 'warn',
        ok: false,
        error: error instanceof Error ? error.message : String(error),
        payload: { originalEvent: event.event, originalSeq: event.seq },
      }),
    )
  })
  return event
}

export function recordDiagnosticError(
  event: string,
  error: unknown,
  input: Omit<DiagnosticInput, 'event' | 'level' | 'ok' | 'error'> = {},
): DiagnosticEvent {
  return recordDiagnostic({
    ...input,
    event,
    level: 'error',
    ok: false,
    error: error instanceof Error ? error.message : String(error),
    payload: {
      ...(isPlainObject(input.payload) ? input.payload : {}),
      stack: error instanceof Error ? error.stack : undefined,
    },
  })
}

export async function withDiagnosticSpan<T>(
  input: DiagnosticInput,
  fn: () => Promise<T>,
): Promise<T> {
  const startedAt = currentMonotonicMs()
  recordDiagnostic({ ...input, event: `${input.event}.started` })

  try {
    const result = await fn()
    const endedAt = currentMonotonicMs()
    recordDiagnostic({
      ...input,
      event: `${input.event}.completed`,
      ok: true,
      durationMs: Math.round(endedAt - startedAt),
    })
    return result
  } catch (error) {
    const endedAt = currentMonotonicMs()
    recordDiagnosticError(`${input.event}.failed`, error, {
      ...input,
      durationMs: Math.round(endedAt - startedAt),
    })
    throw error
  }
}
