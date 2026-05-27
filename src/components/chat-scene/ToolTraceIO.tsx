import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

interface ToolTraceIOProps {
  toolName?: string
  inputJson?: string
  output?: ReactNode
  isError?: boolean
  /**
   * Live stdout/stderr tail while the tool is still running.
   * Rendered as a third "实时输出" block below input. Empty/undefined
   * suppresses the block.
   */
  progressTail?: string
  /** Total bytes captured so far (for the "已收到 N KB" label). */
  progressTotalBytes?: number
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / (1024 * 1024)).toFixed(1)} MB`
}

/**
 * 对单字符串参数的工具（Bash / PowerShell / SkillRun 等），把外层 JSON 包装拆掉，
 * 直接显示命令本身。否则 JSON.stringify 会把 \n 编码成字面字符，多行命令挤在
 * 一行难以阅读。返回 null 表示不适用，调用方 fallback 到原始 inputJson。
 */
function tryUnwrapAsCommand(toolName: string | undefined, inputJson: string): string | null {
  if (!toolName) return null
  // 只对已知"参数主要是一个长字符串"的工具走解包，避免误伤多字段输入。
  const SINGLE_STRING_ARG_TOOLS: Record<string, string> = {
    Bash: 'command',
    PowerShell: 'command',
    Shell: 'command',
  }
  const argKey = SINGLE_STRING_ARG_TOOLS[toolName]
  if (!argKey) return null
  try {
    const parsed = JSON.parse(inputJson) as Record<string, unknown>
    const main = parsed[argKey]
    if (typeof main !== 'string') return null
    // 如果除主字段外还有其他非 trivial 字段，保留原始 JSON 以免丢信息
    const otherKeys = Object.keys(parsed).filter((k) => k !== argKey && parsed[k] != null && parsed[k] !== '')
    if (otherKeys.length === 0) return main
    // 有额外字段：把命令放上面，其他参数附在下面注释里
    const meta = otherKeys
      .map((k) => `# ${k}: ${typeof parsed[k] === 'string' ? parsed[k] : JSON.stringify(parsed[k])}`)
      .join('\n')
    return `${meta}\n${main}`
  } catch {
    return null
  }
}

export function ToolTraceIO({
  toolName,
  inputJson,
  output,
  isError = false,
  progressTail,
  progressTotalBytes,
}: ToolTraceIOProps) {
  const { t } = useTranslation()
  const hasProgress = !!(progressTail && progressTail.length > 0)
  if (!inputJson && !output && !hasProgress) return null

  const unwrappedCommand = inputJson ? tryUnwrapAsCommand(toolName, inputJson) : null

  return (
    <div
      className="flex flex-col gap-3 px-4 pb-4 pt-1"
      style={{ background: 'var(--color-bg-card)' }}
    >
      {inputJson ? (
        <div className="flex flex-col gap-1.5">
          <div className="text-xs font-semibold" style={{ color: 'var(--color-text-muted)' }}>
            {t('toolTrace.input')}
          </div>
          <pre
            className="whitespace-pre-wrap rounded-md p-3 font-mono text-xs leading-relaxed"
            style={{
              background: 'var(--color-bg-code)',
              color: 'var(--color-text-code)',
            }}
          >
            {unwrappedCommand ?? inputJson}
          </pre>
        </div>
      ) : null}
      {hasProgress ? (
        <div className="flex flex-col gap-1.5">
          <div
            className="flex items-baseline justify-between gap-2 text-xs font-semibold"
            style={{ color: 'var(--color-text-muted)' }}
          >
            <span>{t('toolTrace.live', '实时输出')}</span>
            {typeof progressTotalBytes === 'number' && progressTotalBytes > 0 ? (
              <span className="font-normal">
                {t('toolTrace.liveBytes', { bytes: formatBytes(progressTotalBytes), defaultValue: '已收到 {{bytes}}' })}
              </span>
            ) : null}
          </div>
          <pre
            // Tail block: monospace, muted background, scrolls within a max
            // height so a chatty command (npm install) doesn't push the rest
            // of the chat off-screen. Latest line stays visible at the bottom
            // — we render the tail end-of-string, so naturally the most recent
            // output is at the bottom of the pre element. Browsers don't
            // auto-scroll pre tags; that's acceptable because the tail is
            // overwritten in place by each progress event (no append).
            className="max-h-48 overflow-auto whitespace-pre-wrap rounded-md p-3 font-mono text-xs leading-relaxed"
            style={{
              background: 'var(--color-bg-neutral)',
              color: 'var(--color-text-secondary)',
            }}
          >
            {progressTail}
          </pre>
        </div>
      ) : null}
      {output ? (
        <div className="flex flex-col gap-1.5">
          <div className="text-xs font-semibold" style={{ color: 'var(--color-text-muted)' }}>
            {t('toolTrace.output')}
          </div>
          <div
            className="rounded-md p-3 text-xs leading-relaxed"
            style={{
              background: isError
                ? 'var(--color-semantic-red-bg-light)'
                : 'var(--color-bg-neutral)',
              color: isError ? 'var(--color-semantic-red)' : 'var(--color-text-secondary)',
            }}
          >
            {output}
          </div>
        </div>
      ) : null}
    </div>
  )
}
