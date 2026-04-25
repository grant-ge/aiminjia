/**
 * @designSource design.pen#uq6ga ChatComposerCompact
 * @sizing r-18 border 1 bg card padding [16,18,14,18] gap 12
 */
import { useRef, type KeyboardEvent, type ReactNode, type RefObject, type CompositionEventHandler } from 'react'
import { ArrowUp, Blocks, Folder, Mic, Plus, ShieldCheck, Sparkles, X } from 'lucide-react'

import type { ComposerSkillCommand } from '@/stores/chatStore'

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
  permissionLabel?: string
  showProjectButton?: boolean
  tips?: ReactNode
  onKeyDown?: (e: KeyboardEvent<HTMLTextAreaElement>) => void
  isStreaming?: boolean
  onStop?: () => void
  onOpenAttachment?: () => void
  pendingFilesSlot?: ReactNode
  topSlot?: ReactNode
  skillCommand?: ComposerSkillCommand | null
  onClearSkillCommand?: () => void
  textareaRef?: RefObject<HTMLTextAreaElement | null>
  onCompositionStart?: CompositionEventHandler<HTMLTextAreaElement>
  onCompositionEnd?: CompositionEventHandler<HTMLTextAreaElement>
  onPermissionClick?: () => void
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
  permissionLabel = '完全访问权限',
  showProjectButton = true,
  tips,
  onKeyDown,
  isStreaming = false,
  onStop,
  onOpenAttachment,
  pendingFilesSlot,
  topSlot,
  skillCommand,
  onClearSkillCommand,
  textareaRef,
  onCompositionStart,
  onCompositionEnd,
  onPermissionClick,
}: ChatComposerCompactProps) {
  const internalRef = useRef<HTMLTextAreaElement>(null)
  const ref = textareaRef ?? internalRef

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    onKeyDown?.(e)
    if (e.defaultPrevented) return
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      if (!submitDisabled && value.trim()) onSubmit(value)
    }
  }

  return (
    <div className="flex w-full flex-col gap-2">
      <div
        data-testid="composer-root"
        className="flex w-full flex-col rounded-[18px] border border-border bg-card px-4 pb-1 pt-4"
      >
        {topSlot}
        {pendingFilesSlot}
        {skillCommand ? (
          <div className="mb-2 flex items-center gap-2">
            <div
              className="group inline-flex max-w-full items-center gap-2 rounded-full border px-3 py-1.5 text-[13px] font-semibold shadow-[0_8px_24px_rgba(212,168,67,0.12)]"
              style={{
                borderColor: 'var(--color-accent-border)',
                background: 'var(--color-accent-subtle)',
                color: 'var(--color-accent-700)',
              }}
            >
              <span className="flex h-5 w-5 items-center justify-center rounded-full text-white" style={{ background: 'var(--color-accent)' }}>
                <Sparkles className="h-3.5 w-3.5" />
              </span>
              <span className="truncate">{skillCommand.label}</span>
              <span className="rounded-md bg-white/70 px-1.5 py-0.5 text-[11px] font-medium" style={{ color: 'var(--color-accent-600)' }}>
                {skillCommand.command}
              </span>
              {onClearSkillCommand ? (
                <button
                  type="button"
                  aria-label={`移除技能 ${skillCommand.label}`}
                  onClick={onClearSkillCommand}
                  className="ml-0.5 flex h-5 w-5 items-center justify-center rounded-full transition-colors hover:bg-[var(--color-accent-muted)]"
                  style={{ color: 'var(--color-accent-700)' }}
                >
                  <X className="h-3 w-3" />
                </button>
              ) : null}
            </div>
          </div>
        ) : null}
        <textarea
          ref={ref}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={handleKeyDown}
          onCompositionStart={onCompositionStart}
          onCompositionEnd={onCompositionEnd}
          placeholder={placeholder}
          rows={1}
          className="w-full resize-none bg-transparent text-[13px] text-foreground outline-none placeholder:text-muted-foreground"
          style={{ minHeight: '40px' }}
        />
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-0">
            <button
              type="button"
              aria-label="添加附件"
              onClick={onOpenAttachment}
              className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted"
            >
              <Plus className="h-4 w-4" />
            </button>
            <button
              type="button"
              onClick={onOpenSkill}
              aria-label={skillCommand ? `打开技能选择，当前已加载技能 ${skillCommand.label}` : '打开技能选择'}
              aria-pressed={Boolean(skillCommand)}
              className={skillCommand
                ? 'flex items-center gap-1.5 rounded-md px-2 py-1 text-[13px] font-semibold transition-colors hover:bg-[var(--color-accent-muted)]'
                : 'flex items-center gap-1.5 rounded-md px-2 py-1 text-[13px] text-muted-foreground transition-colors hover:bg-muted'
              }
              style={skillCommand ? { background: 'var(--color-accent-subtle)', color: 'var(--color-accent-700)' } : undefined}
            >
              <Blocks className="h-3.5 w-3.5" />
              <span>{skillCommand ? '技能已加载' : '技能'}</span>
            </button>
            <button
              type="button"
              onClick={onPermissionClick}
              className="flex items-center gap-1.5 rounded-md px-2 py-1 text-[13px] text-muted-foreground transition-colors hover:bg-muted"
            >
              <ShieldCheck className="h-3.5 w-3.5" />
              <span>{permissionLabel}</span>
            </button>
            {showProjectButton ? (
              <button
                type="button"
                onClick={onPickProject}
                className="flex items-center gap-1.5 rounded-md px-2 py-1 text-[13px] text-muted-foreground transition-colors hover:bg-muted"
              >
                <Folder className="h-3.5 w-3.5" />
                <span>{projectLabel}</span>
              </button>
            ) : null}
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
              aria-label={isStreaming ? '停止' : '发送'}
              onClick={() => {
                if (isStreaming) {
                  onStop?.()
                  return
                }
                if (!submitDisabled && value.trim()) onSubmit(value)
              }}
              disabled={isStreaming ? false : submitDisabled}
              className={
                !isStreaming && (submitDisabled || !value.trim())
                  ? 'flex h-8 w-8 items-center justify-center rounded-full text-muted-foreground'
                  : 'flex h-8 w-8 items-center justify-center rounded-full bg-primary text-primary-foreground transition-colors hover:opacity-90'
              }
              style={!isStreaming && (submitDisabled || !value.trim()) ? { backgroundColor: '#D4D4D8' } : undefined}
            >
              {isStreaming ? (
                <span className="block h-3.5 w-3.5 rounded-[2px] bg-current" />
              ) : (
                <ArrowUp className="h-4 w-4" />
              )}
            </button>
          </div>
        </div>
      </div>
      {tips ? (
        <div data-testid="composer-tips" className="flex items-center justify-between gap-3 px-3 text-[11px] text-muted-foreground">
          {tips}
        </div>
      ) : null}
    </div>
  )
}
