/**
 * @designSource design.pen#uq6ga ChatComposerCompact
 * @sizing r-18 border 1 bg card padding [16,18,14,18] gap 12
 */
import { useRef } from 'react'
import { ArrowUp, Blocks, Folder, Mic, Plus, ShieldCheck } from 'lucide-react'

interface ChatComposerCompactProps {
  value: string
  onChange: (v: string) => void
  onSubmit: (v: string) => void
  submitDisabled?: boolean
  placeholder?: string
  onOpenSkill?: () => void
  onPickProject?: () => void
  projectLabel?: string
  modelLabel?: string
}

export function ChatComposerCompact({
  value,
  onChange,
  onSubmit,
  submitDisabled = false,
  placeholder = '继续追问、修改口径，或让 AI 直接帮你创建后续任务...',
  onOpenSkill,
  onPickProject,
  projectLabel = 'Desktop',
  modelLabel = '标准',
}: ChatComposerCompactProps) {
  const ref = useRef<HTMLTextAreaElement>(null)

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      if (!submitDisabled && value.trim()) onSubmit(value)
    }
  }

  return (
    <div
      data-testid="composer-root"
      className="flex w-full flex-col gap-3 rounded-[18px] border border-border bg-card px-4 pb-3.5 pt-4"
    >
      <textarea
        ref={ref}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={handleKeyDown}
        placeholder={placeholder}
        rows={1}
        className="w-full resize-none bg-transparent text-[13px] text-foreground outline-none placeholder:text-muted-foreground"
        style={{ minHeight: '40px' }}
      />
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <button
            type="button"
            aria-label="添加附件"
            className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted"
          >
            <Plus className="h-4 w-4" />
          </button>
          <button
            type="button"
            onClick={onOpenSkill}
            className="flex items-center gap-1.5 rounded-md px-2 py-1 text-[13px] text-muted-foreground transition-colors hover:bg-muted"
          >
            <Blocks className="h-3.5 w-3.5" />
            <span>技能</span>
          </button>
          <button
            type="button"
            className="flex items-center gap-1.5 rounded-md px-2 py-1 text-[13px] text-muted-foreground transition-colors hover:bg-muted"
          >
            <ShieldCheck className="h-3.5 w-3.5" />
            <span>完全访问权限</span>
          </button>
          <button
            type="button"
            onClick={onPickProject}
            className="flex items-center gap-1.5 rounded-md px-2 py-1 text-[13px] text-muted-foreground transition-colors hover:bg-muted"
          >
            <Folder className="h-3.5 w-3.5" />
            <span>{projectLabel}</span>
          </button>
        </div>
        <div className="flex items-center gap-3">
          <button
            type="button"
            className="flex items-center gap-1 rounded-md px-2 py-1 text-[13px] text-muted-foreground transition-colors hover:bg-muted"
          >
            <span>{modelLabel}</span>
          </button>
          <button
            type="button"
            aria-label="语音输入"
            className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted"
          >
            <Mic className="h-4 w-4" />
          </button>
          <button
            type="button"
            aria-label="发送"
            onClick={() => { if (!submitDisabled && value.trim()) onSubmit(value) }}
            disabled={submitDisabled}
            className={
              submitDisabled || !value.trim()
                ? 'flex h-8 w-8 items-center justify-center rounded-full text-muted-foreground'
                : 'flex h-8 w-8 items-center justify-center rounded-full bg-primary text-primary-foreground transition-colors hover:opacity-90'
            }
            style={submitDisabled || !value.trim() ? { backgroundColor: '#D4D4D8' } : undefined}
          >
            <ArrowUp className="h-4 w-4" />
          </button>
        </div>
      </div>
    </div>
  )
}
