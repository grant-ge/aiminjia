import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from 'react'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { ArrowUp, Blocks, Folder, Plus, Sparkles, X } from 'lucide-react'
import { EditorContent, useEditor } from '@tiptap/react'
import type { Editor } from '@tiptap/react'
import { buildComposerExtensions } from './composerSchema'
import { serializeComposerDoc } from './serializer'
import { parseMarkdownToComposerJson } from './parseMarkdown'
import type { ComposerAttachmentToken, ComposerJsonNode, RichComposerSubmitPayload } from './types'

export interface ComposerSkillCommand {
  command: string
  label: string
  id?: string
}

export interface RichComposerProps {
  placeholder?: string
  disabled?: boolean
  isStreaming?: boolean
  autoFocus?: boolean
  initialMarkdown?: string
  clearOnSubmit?: boolean

  onSubmit: (payload: RichComposerSubmitPayload) => void | Promise<void>
  onStop?: () => void

  topSlot?: ReactNode
  tips?: ReactNode

  onOpenSkill?: () => void
  skillCommand?: ComposerSkillCommand | null
  onClearSkillCommand?: () => void

  projectLabel?: string
  onPickProject?: () => void
  showProjectButton?: boolean

  onOpenAttachment?: () => void
}

export interface RichComposerHandle {
  focus: () => void
  insertAttachmentTokens: (tokens: ComposerAttachmentToken[]) => void
  clear: () => void
  getEditor: () => Editor | null
}

export const RichComposer = forwardRef<RichComposerHandle, RichComposerProps>(function RichComposer(
  {
    placeholder = '',
    disabled = false,
    isStreaming = false,
    autoFocus = false,
    initialMarkdown,
    clearOnSubmit = false,
    onSubmit,
    onStop,
    topSlot,
    tips,
    onOpenSkill,
    skillCommand,
    onClearSkillCommand,
    projectLabel = 'Desktop',
    onPickProject,
    showProjectButton = true,
    onOpenAttachment,
  },
  ref,
) {
  const { t } = useTranslation()
  const isComposingRef = useRef(false)
  const submittingRef = useRef(false)
  // Force a re-render when content/submit state changes so the send button's disabled state stays accurate.
  const [, forceTick] = useState(0)
  const editor = useEditor({
    extensions: buildComposerExtensions({ placeholder }),
    content: initialMarkdown
      ? (parseMarkdownToComposerJson(initialMarkdown) as unknown as object)
      : undefined,
    editable: !disabled,
    autofocus: autoFocus ? 'end' : false,
    onUpdate: () => forceTick((n) => n + 1),
  })

  useEffect(() => {
    editor?.setEditable(!disabled)
  }, [editor, disabled])

  const trySubmit = useCallback(async () => {
    if (!editor) return
    if (disabled) return
    if (submittingRef.current) return
    // Note: isStreaming does NOT block submission. When a turn is in flight,
    // the backend PendingQueueManager will buffer the message and merge it
    // into the next turn after the current one ends + debounce window.
    const json = editor.getJSON() as unknown as ComposerJsonNode
    const payload = serializeComposerDoc(json)
    if (payload.isEmpty) return
    submittingRef.current = true
    // Clear synchronously so the user can start typing the next message while
    // onSubmit is still in flight. If onSubmit rejects we restore the prior
    // content (markdown round-trip via parseMarkdownToComposerJson keeps text /
    // attachment tokens but loses richer marks — acceptable for a failure path).
    if (clearOnSubmit) editor.commands.clearContent()
    // Release submittingRef on the next microtask. The earlier "await onSubmit
    // then release in finally" approach deadlocked the composer for the entire
    // streaming turn: the SentDirectly IPC only resolves after stream:done, so
    // every subsequent Enter during streaming was silently swallowed by the
    // ref. submittingRef is a thin double-fire guard only — the PendingQueue
    // backend is the real serialization point.
    queueMicrotask(() => {
      submittingRef.current = false
      forceTick((n) => n + 1)
    })
    try {
      await onSubmit(payload)
    } catch {
      if (clearOnSubmit) {
        editor.commands.setContent(parseMarkdownToComposerJson(payload.markdown))
      }
    }
  }, [editor, disabled, onSubmit, clearOnSubmit])

  useEffect(() => {
    if (!editor) return
    const dom = editor.view.dom
    const onCompositionStart = () => {
      isComposingRef.current = true
    }
    const onCompositionEnd = () => {
      window.setTimeout(() => {
        isComposingRef.current = false
      }, 50)
    }
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== 'Enter' || e.shiftKey) return
      if (isComposingRef.current || e.isComposing) return
      // Capture phase + stopImmediatePropagation: beat ProseMirror's own
      // keydown handler so Enter never reaches the default newline insertion.
      e.preventDefault()
      e.stopImmediatePropagation()
      void trySubmit()
    }
    dom.addEventListener('compositionstart', onCompositionStart)
    dom.addEventListener('compositionend', onCompositionEnd)
    // useCapture=true → fires before ProseMirror's bubble-phase listener.
    dom.addEventListener('keydown', onKeyDown, true)
    return () => {
      dom.removeEventListener('compositionstart', onCompositionStart)
      dom.removeEventListener('compositionend', onCompositionEnd)
      dom.removeEventListener('keydown', onKeyDown, true)
    }
  }, [editor, trySubmit])

  useImperativeHandle(
    ref,
    () => ({
      focus: () => {
        editor?.commands.focus('end')
      },
      insertAttachmentTokens: (tokens) => {
        editor?.commands.insertAttachmentTokens(tokens)
      },
      clear: () => {
        editor?.commands.clearContent()
      },
      getEditor: () => editor ?? null,
    }),
    [editor],
  )

  const isEmpty = !editor || editor.isEmpty
  // Send button is disabled when there's nothing to send. During streaming
  // we still allow send (it queues via PendingQueueManager).
  const sendDisabled = disabled || isEmpty || submittingRef.current

  return (
    <div className="flex w-full flex-col gap-2">
      <div
        data-testid="composer-root"
        className="flex w-full flex-col rounded-xl border border-border bg-card px-4 pb-1 pt-4"
      >
        {topSlot}
        {skillCommand ? (
          <div className="-mt-2 mb-1 flex items-center">
            <div
              className="inline-flex max-w-full items-center gap-1.5 rounded-full border px-2 py-1 text-xs"
              style={{
                borderColor: 'var(--color-accent-border)',
                background: 'var(--color-accent-subtle)',
                color: 'var(--color-accent-700)',
              }}
            >
              <Sparkles
                className="h-3 w-3 shrink-0"
                style={{ color: 'var(--color-accent)' }}
              />
              <span className="truncate font-medium">{skillCommand.label}</span>
              <span
                className="shrink-0 rounded px-1 text-[11px]"
                style={{
                  background: 'var(--color-accent-muted)',
                  color: 'var(--color-accent-600)',
                }}
              >
                {skillCommand.command}
              </span>
              {onClearSkillCommand ? (
                <button
                  type="button"
                  aria-label={t('composer.removeSkill', { name: skillCommand.label })}
                  onClick={onClearSkillCommand}
                  className="ml-0.5 shrink-0 rounded p-0.5 transition-colors hover:bg-[var(--color-accent-muted)]"
                  style={{ color: 'var(--color-accent-700)' }}
                >
                  <X className="h-3 w-3" />
                </button>
              ) : null}
            </div>
          </div>
        ) : null}
        <EditorContent
          editor={editor}
          className="min-h-[40px] w-full text-sm text-foreground [&_.ProseMirror]:outline-none [&_.ProseMirror_p.is-editor-empty:first-child]:before:pointer-events-none [&_.ProseMirror_p.is-editor-empty:first-child]:before:content-[attr(data-placeholder)] [&_.ProseMirror_p.is-editor-empty:first-child]:before:text-muted-foreground [&_.ProseMirror_p.is-editor-empty:first-child]:before:float-left [&_.ProseMirror_p.is-editor-empty:first-child]:before:h-0 [&_.ProseMirror_a]:text-primary [&_.ProseMirror_a]:underline [&_.ProseMirror_a]:underline-offset-2 [&_.ProseMirror_a]:cursor-pointer [&_.ProseMirror_strong]:font-semibold [&_.ProseMirror_em]:italic [&_.ProseMirror_code]:rounded [&_.ProseMirror_code]:bg-muted [&_.ProseMirror_code]:px-1 [&_.ProseMirror_code]:text-[0.85em] [&_.ProseMirror_pre]:overflow-x-auto [&_.ProseMirror_pre]:rounded [&_.ProseMirror_pre]:bg-muted [&_.ProseMirror_pre]:p-2 [&_.ProseMirror_pre]:text-xs [&_.ProseMirror_pre_code]:bg-transparent [&_.ProseMirror_pre_code]:p-0 [&_.ProseMirror_ul]:list-disc [&_.ProseMirror_ul]:pl-5 [&_.ProseMirror_ol]:list-decimal [&_.ProseMirror_ol]:pl-5 [&_.ProseMirror_blockquote]:border-l-2 [&_.ProseMirror_blockquote]:border-border [&_.ProseMirror_blockquote]:pl-3 [&_.ProseMirror_blockquote]:opacity-90"
        />
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-0">
            <button
              type="button"
              data-aijia-composer-plus
              aria-label={t('composer.addAttachment')}
              onClick={onOpenAttachment}
              disabled={disabled}
              className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted disabled:opacity-40"
            >
              <Plus className="h-4 w-4" />
            </button>
            <button
              type="button"
              onClick={onOpenSkill}
              disabled={disabled}
              aria-label={
                skillCommand
                  ? t('composer.openSkillPickerWithSkill', { name: skillCommand.label })
                  : t('composer.openSkillPicker')
              }
              aria-pressed={Boolean(skillCommand)}
              className={
                skillCommand
                  ? 'flex items-center gap-1.5 rounded-md px-2 py-1 text-sm font-semibold transition-colors hover:bg-[var(--color-accent-muted)] disabled:opacity-40'
                  : 'flex items-center gap-1.5 rounded-md px-2 py-1 text-sm text-muted-foreground transition-colors hover:bg-muted disabled:opacity-40'
              }
              style={
                skillCommand
                  ? { background: 'var(--color-accent-subtle)', color: 'var(--color-accent-700)' }
                  : undefined
              }
            >
              <Blocks className="h-3.5 w-3.5" />
              <span>{skillCommand ? t('composer.skillLoaded') : t('composer.skill')}</span>
            </button>
            {showProjectButton ? (
              <button
                type="button"
                onClick={onPickProject}
                disabled={disabled}
                className="flex items-center gap-1.5 rounded-md px-2 py-1 text-sm text-muted-foreground transition-colors hover:bg-muted disabled:opacity-40"
              >
                <Folder className="h-3.5 w-3.5" />
                <span>{projectLabel}</span>
              </button>
            ) : null}
          </div>
          <div className="flex items-center gap-3">
            {/*
              Streaming state: show ONLY the stop button.
              The user can still queue a new message by pressing Enter — the
              backend PendingQueueManager buffers it for the next turn. We
              intentionally hide the send button during streaming to keep the
              "current turn" UX focused on the stop action.
            */}
            {isStreaming ? (
              <button
                type="button"
                aria-label={t('composer.stop')}
                onClick={() => onStop?.()}
                className="flex h-7 w-7 items-center justify-center rounded-full bg-primary text-primary-foreground transition-colors hover:opacity-90"
              >
                <span className="block h-3 w-3 rounded-[2px] bg-current" />
              </button>
            ) : (
              <button
                type="button"
                aria-label={t('composer.send')}
                onClick={() => {
                  void trySubmit()
                }}
                disabled={sendDisabled}
                className={
                  sendDisabled
                    ? 'flex h-7 w-7 items-center justify-center rounded-full bg-muted text-muted-foreground'
                    : 'flex h-7 w-7 items-center justify-center rounded-full bg-primary text-primary-foreground transition-colors hover:opacity-90'
                }
              >
                <ArrowUp className="h-4 w-4" />
              </button>
            )}
          </div>
        </div>
      </div>
      {tips ? (
        <div
          data-testid="composer-tips"
          className="flex items-center justify-between gap-3 px-3 text-xs text-muted-foreground"
        >
          {tips}
        </div>
      ) : null}
    </div>
  )
})
