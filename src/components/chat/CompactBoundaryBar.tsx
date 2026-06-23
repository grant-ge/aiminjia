import { useState } from 'react'
import { Archive, ChevronDown, ChevronRight } from 'lucide-react'
import { Button } from '@/components/ui/button'

interface CompactBoundaryBarProps {
  preTokens?: number
  postTokens?: number
  tokensSaved?: number
  messagesSummarized?: number
}

function formatTokens(value: number | undefined): string | null {
  if (typeof value !== 'number') return null
  return new Intl.NumberFormat('zh-CN').format(value)
}

export function CompactBoundaryBar({
  preTokens,
  postTokens,
  tokensSaved,
  messagesSummarized,
}: CompactBoundaryBarProps) {
  const [open, setOpen] = useState(false)
  const saved = formatTokens(tokensSaved)
  const pre = formatTokens(preTokens)
  const post = formatTokens(postTokens)
  const summarized = formatTokens(messagesSummarized)
  const Chevron = open ? ChevronDown : ChevronRight

  return (
    <div className="flex justify-center">
      <div
        className="w-full max-w-xl rounded-md border border-border bg-muted/40 text-muted-foreground"
        data-aijia-compact-boundary
      >
        <Button unstyled
          type="button"
          aria-expanded={open}
          data-aijia-compact-boundary-toggle
          className="flex w-full items-center gap-2 px-3 py-2 text-left text-xs"
          onClick={() => setOpen((value) => !value)}
        >
          <Chevron className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
          <Archive className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
          <span className="font-medium text-foreground">对话已压缩</span>
          {saved ? <span>节省 {saved} tokens</span> : null}
        </Button>
        {open ? (
          <div className="grid grid-cols-3 gap-2 border-t border-border px-3 py-2 text-xs">
            <div>
              <div className="text-muted-foreground">压缩前</div>
              <div className="text-foreground">{pre ?? '-'}</div>
            </div>
            <div>
              <div className="text-muted-foreground">压缩后</div>
              <div className="text-foreground">{post ?? '-'}</div>
            </div>
            <div>
              <div className="text-muted-foreground">摘要消息</div>
              <div className="text-foreground">{summarized ?? '-'}</div>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  )
}
