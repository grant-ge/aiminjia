import { useState, type ReactNode, type CSSProperties } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'

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

const DEFAULT_VISIBLE_LINES = 5
// 字符数兜底：当文本没多少真换行但单行/全文超长时（典型场景：tool 输出的
// JSON 把 content 字段塞成一行带字面 \n 的超长字符串），靠真换行计数无法触
// 发折叠，整段视觉上靠 whitespace-pre-wrap 自动 wrap 撑成几十显示行。这里
// 按字符数兜底，约对应 5 行 wrap 后的视觉高度（chat 列宽 ~700px、text-xs
// monospace 约 95 char/行 × 5 ≈ 475，取保守 500）。
const DEFAULT_VISIBLE_CHARS = 500

/** 把多行字符串包装成可折叠块：默认显示前 N 行 / N 字符，超过则提供
 *  "显示更多/收起"。外层 block 级 w-full 占满父容器宽度（内容超短时也撑满，
 *  避免出现"{...}"挤成小药丸）。 */
function CollapsiblePre({
  text,
  pre = true,
  className,
  style,
}: {
  text: string
  /** true 渲染为 <pre>（保留空白/换行）；false 渲染为 <div whitespace-pre-wrap>。 */
  pre?: boolean
  className: string
  style: CSSProperties
}) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(false)
  const lines = text.split('\n')
  const lineOverflow = lines.length > DEFAULT_VISIBLE_LINES
  const charOverflow = text.length > DEFAULT_VISIBLE_CHARS
  const overflow = lineOverflow || charOverflow

  let visible: string
  if (!overflow || expanded) {
    visible = text
  } else if (lineOverflow) {
    visible = lines.slice(0, DEFAULT_VISIBLE_LINES).join('\n')
  } else {
    // char overflow only：在 \n 边界附近截断比硬切更顺眼
    const cutAt = text.lastIndexOf('\n', DEFAULT_VISIBLE_CHARS)
    visible = text.slice(0, cutAt > 0 ? cutAt : DEFAULT_VISIBLE_CHARS) + '…'
  }
  const Content = pre ? 'pre' : 'div'
  // w-full 让 Content 占满父容器宽度（即便文本只有"{...}"也撑成完整 block，
  // 不会挤成内容宽度的小药丸）。
  return (
    <div className="flex w-full flex-col items-start">
      <Content className={`${className} w-full`} style={style}>
        {visible}
      </Content>
      {overflow ? (
        <Button unstyled
          type="button"
          onClick={() => setExpanded((e) => !e)}
          className="mt-1 text-xs text-muted-foreground hover:text-foreground"
        >
          {expanded
            ? t('toolTrace.collapse')
            : lineOverflow
              ? t('toolTrace.showMoreLines', { count: lines.length - DEFAULT_VISIBLE_LINES })
              : t('toolTrace.showMore')}
        </Button>
      ) : null}
    </div>
  )
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
  // output 通常是 truncateOutput 产出的 string，但类型签名兼容 ReactNode。
  // 仅 string 时走折叠；其他 ReactNode 直接渲染（保留弹性）。
  const outputIsString = typeof output === 'string'

  return (
    <div
      className="flex flex-col items-start gap-3 px-4 pb-4 pt-1"
      style={{ background: 'var(--color-bg-card)' }}
    >
      {inputJson ? (
        <div className="flex w-full flex-col items-start gap-1.5">
          <div className="text-xs font-semibold" style={{ color: 'var(--color-text-muted)' }}>
            {t('toolTrace.input')}
          </div>
          <CollapsiblePre
            text={unwrappedCommand ?? inputJson}
            className="whitespace-pre-wrap rounded-md p-3 font-mono text-xs leading-relaxed"
            style={{
              background: 'var(--color-bg-code)',
              color: 'var(--color-text-code)',
            }}
          />
        </div>
      ) : null}
      {hasProgress ? (
        <div className="flex w-full flex-col items-start gap-1.5">
          <div
            className="flex items-baseline justify-between gap-2 text-xs font-semibold"
            style={{ color: 'var(--color-text-muted)' }}
          >
            <span>{t('toolTrace.live')}</span>
            {typeof progressTotalBytes === 'number' && progressTotalBytes > 0 ? (
              <span className="font-normal">
                {t('toolTrace.liveBytes', { bytes: formatBytes(progressTotalBytes) })}
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
        <div className="flex w-full flex-col items-start gap-1.5">
          <div className="text-xs font-semibold" style={{ color: 'var(--color-text-muted)' }}>
            {t('toolTrace.output')}
          </div>
          {outputIsString ? (
            <CollapsiblePre
              text={output as string}
              pre={false}
              className="whitespace-pre-wrap rounded-md p-3 text-xs leading-relaxed"
              style={{
                background: isError
                  ? 'var(--color-semantic-red-bg-light)'
                  : 'var(--color-bg-neutral)',
                color: isError ? 'var(--color-semantic-red)' : 'var(--color-text-secondary)',
              }}
            />
          ) : (
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
          )}
        </div>
      ) : null}
    </div>
  )
}
