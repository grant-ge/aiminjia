import { Check, Sparkles, X } from 'lucide-react'

import { LLM_PROVIDER_LABELS, PROVIDER_CAPABILITIES, type LlmProvider } from '@/types/settings'

const MODEL_OPTIONS: Array<{
  value: LlmProvider
  badge: string
  tone: {
    bg: string
    border: string
    text: string
    muted: string
  }
}> = [
  {
    value: 'deepseek-v3',
    badge: '默认',
    tone: {
      bg: 'linear-gradient(180deg, rgba(245,250,255,0.96), rgba(238,246,255,0.96))',
      border: 'rgba(116, 168, 255, 0.24)',
      text: '#0F3D87',
      muted: '#4C6EA8',
    },
  },
  {
    value: 'qwen-plus',
    badge: '效率',
    tone: {
      bg: 'linear-gradient(180deg, rgba(246,251,248,0.98), rgba(236,246,241,0.98))',
      border: 'rgba(90, 170, 126, 0.26)',
      text: '#23563A',
      muted: '#4C7A62',
    },
  },
  {
    value: 'volcano',
    badge: '企业',
    tone: {
      bg: 'linear-gradient(180deg, rgba(255,249,242,0.98), rgba(251,240,225,0.98))',
      border: 'rgba(212, 148, 68, 0.28)',
      text: '#7A4715',
      muted: '#946539',
    },
  },
  {
    value: 'openai',
    badge: '通用',
    tone: {
      bg: 'linear-gradient(180deg, rgba(245,248,251,0.98), rgba(236,241,247,0.98))',
      border: 'rgba(107, 129, 160, 0.26)',
      text: '#24415F',
      muted: '#5A728F',
    },
  },
  {
    value: 'claude',
    badge: '长文本',
    tone: {
      bg: 'linear-gradient(180deg, rgba(252,247,241,0.98), rgba(247,238,228,0.98))',
      border: 'rgba(177, 125, 84, 0.26)',
      text: '#6D4325',
      muted: '#8D6245',
    },
  },
  {
    value: 'custom',
    badge: '自定义',
    tone: {
      bg: 'linear-gradient(180deg, rgba(248,246,251,0.98), rgba(241,237,247,0.98))',
      border: 'rgba(132, 112, 180, 0.24)',
      text: '#4A356F',
      muted: '#705A93',
    },
  },
]

interface ModelPickerPopoverProps {
  open: boolean
  value: LlmProvider
  onChange: (value: LlmProvider) => void
  onClose: () => void
}

export function ModelPickerPopover({ open, value, onChange, onClose }: ModelPickerPopoverProps) {
  if (!open) return null

  return (
    <div
      // spec §5 — popover-level shadow token; was hardcoded slate-color dropshadow.
      className="absolute right-0 bottom-[calc(100%+10px)] z-50 flex h-[400px] w-[min(620px,calc(100vw-48px))] flex-col overflow-hidden rounded-md border border-border bg-card shadow-[var(--shadow-popover)]"
      onMouseDown={(event) => {
        event.preventDefault()
      }}
    >
      <div className="flex items-start justify-between gap-4 border-b border-border px-5 py-4">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-md font-semibold text-foreground">
            <Sparkles className="h-4 w-4 text-[var(--color-accent)]" />
            <span>选择模型</span>
          </div>
          <p className="mt-1 text-xs leading-5 text-muted-foreground">
            固定弹层宽高，列表在卡片内部滚动，避免页面和弹层一起跳动。
          </p>
        </div>
        <button
          type="button"
          aria-label="关闭模型弹窗"
          onClick={onClose}
          className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
        >
          <X className="h-4 w-4" />
        </button>
      </div>

      <div className="min-h-0 flex-1 px-4 py-4">
        <div
          data-testid="model-popover-grid-box"
          className="grid h-full min-h-0 grid-cols-2 gap-3 overflow-y-auto overscroll-contain pr-1"
        >
          {MODEL_OPTIONS.map((option) => {
            const selected = option.value === value
            return (
              <button
                key={option.value}
                type="button"
                onClick={() => {
                  onChange(option.value)
                  onClose()
                }}
                className="relative flex h-[132px] flex-col rounded-md border p-4 text-left transition-colors border-border"
                style={{
                  background: option.tone.bg,
                  borderColor: selected ? 'var(--color-accent)' : option.tone.border,
                  boxShadow: selected ? 'inset 0 0 0 1px var(--color-accent)' : 'none',
                }}
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="text-md font-semibold" style={{ color: option.tone.text }}>
                      {LLM_PROVIDER_LABELS[option.value]}
                    </div>
                    <div className="mt-1 inline-flex rounded-md px-2 py-0.5 text-xs font-medium" style={{ background: 'rgba(255,255,255,0.74)', color: option.tone.muted }}>
                      {option.badge}
                    </div>
                  </div>
                  <span
                    className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-border"
                    style={{
                      borderColor: selected ? 'var(--color-accent)' : 'rgba(255,255,255,0.72)',
                      background: selected ? 'var(--color-accent)' : 'rgba(255,255,255,0.62)',
                      color: selected ? 'white' : 'transparent',
                    }}
                  >
                    <Check className="h-3.5 w-3.5" />
                  </span>
                </div>
                <p className="mt-4 line-clamp-2 text-xs leading-5" style={{ color: option.tone.muted }}>
                  {PROVIDER_CAPABILITIES[option.value].modelsDesc}
                </p>
                <div className="mt-auto text-xs font-medium" style={{ color: option.tone.text }}>
                  {PROVIDER_CAPABILITIES[option.value].hasReasoning ? '支持推理模型' : '标准模型入口'}
                </div>
              </button>
            )
          })}
        </div>
      </div>

      <div className="flex shrink-0 items-center justify-between border-t border-border px-5 py-3 text-xs text-muted-foreground">
        <span>当前模型：{LLM_PROVIDER_LABELS[value]}</span>
        <span>列表滚动已限制在卡片区内部</span>
      </div>
    </div>
  )
}
