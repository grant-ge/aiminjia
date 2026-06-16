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
import { AlertTriangle, ArrowUp, Blocks, Check, ChevronDown, Folder, Plus, Shield, ShieldCheck, Sparkles, X } from 'lucide-react'
import { EditorContent, useEditor } from '@tiptap/react'
import type { Editor } from '@tiptap/react'
import { buildComposerExtensions } from './composerSchema'
import { serializeComposerDoc } from './serializer'
import { parseMarkdownToComposerJson } from './parseMarkdown'
import type { ComposerAttachmentToken, ComposerJsonNode, ComposerSkillToken, RichComposerSubmitPayload } from './types'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import type { PermissionMode } from '@/lib/tauri'

// `/` should open the skill picker only where a real slash-command could
// start: an empty doc, or right after whitespace. Anywhere else (mid-word,
// mid-URL) we let the slash type through as a normal character.
function isSlashBoundary(editor: Editor | null): boolean {
  if (!editor) return false
  const { selection, doc } = editor.state
  if (!selection.empty) return false
  const $from = selection.$from
  if (doc.textContent.length === 0) return true
  if ($from.parentOffset === 0) return true
  const before = $from.parent.textBetween(Math.max(0, $from.parentOffset - 1), $from.parentOffset, undefined, '￼')
  return /\s/.test(before)
}

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
  permissionMode?: PermissionMode
  onPermissionModeChange?: (mode: PermissionMode) => void

  projectLabel?: string
  onPickProject?: () => void
  showProjectButton?: boolean

  onOpenAttachment?: () => void
  skillTokens?: ComposerSkillToken[]
  /** Extra classes appended to the outer rounded-md container. Caller-controlled
   * styling (e.g. shadow for the in-chat composer vs. flat for home composer). */
  containerClassName?: string
  /** When true, caps the editor content area at 200px and enables internal scrolling. */
  limitEditorHeight?: boolean
}

export interface RichComposerHandle {
  focus: () => void
  insertAttachmentTokens: (tokens: ComposerAttachmentToken[]) => void
  insertSkillToken: (token: ComposerSkillToken) => void
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
    permissionMode = 'default',
    onPermissionModeChange,
    projectLabel = 'Desktop',
    onPickProject,
    showProjectButton = true,
    onOpenAttachment,
    skillTokens,
    containerClassName,
    limitEditorHeight = false,
  },
  ref,
) {
  const { t } = useTranslation()
  const isComposingRef = useRef(false)
  const submittingRef = useRef(false)
  // Force a re-render when content/submit state changes so the send button's disabled state stays accurate.
  const [, forceTick] = useState(0)
  const editor = useEditor({
    extensions: buildComposerExtensions({ placeholder, skills: skillTokens ?? [] }),
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
      if (isComposingRef.current || e.isComposing) return
      // Slash shortcut: pop the skill picker when "/" is typed at a position
      // where it would naturally start a command — empty doc or whitespace
      // boundary. We swallow the keystroke (no "/" lands in the editor) and
      // delegate to the caller's onOpenSkill so home/chat composers reuse
      // the same popover they already wire to the Blocks button.
      if (
        e.key === '/' &&
        !e.shiftKey &&
        !e.ctrlKey &&
        !e.metaKey &&
        !e.altKey &&
        onOpenSkill
      ) {
        if (isSlashBoundary(editor)) {
          e.preventDefault()
          e.stopImmediatePropagation()
          onOpenSkill()
          return
        }
      }
      if (e.key !== 'Enter' || e.shiftKey) return
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
  }, [editor, trySubmit, onOpenSkill])

  useImperativeHandle(
    ref,
    () => ({
      focus: () => {
        editor?.commands.focus('end')
      },
      insertAttachmentTokens: (tokens) => {
        editor?.commands.insertAttachmentTokens(tokens)
      },
      insertSkillToken: (token) => {
        editor?.commands.insertSkillToken(token)
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
  const stopIcon = <span className="block h-3 w-3 rounded-md bg-current" />
  const fullAccess = permissionMode === 'fullAccess'

  return (
    <div className="relative z-10 flex w-full flex-col gap-2">
      <div
        data-testid="composer-root"
        className={`flex w-full flex-col rounded-md border border-border bg-card px-4 pb-1 pt-4${containerClassName ? ` ${containerClassName}` : ''}`}
      >
        {topSlot}
        {skillCommand ? (
          <div className="-mt-2 mb-1 flex items-center">
            <div
              className="inline-flex max-w-full items-center gap-1.5 rounded-md border px-2 py-1 text-xs"
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
                className="shrink-0 rounded-md px-1 text-[11px]"
                style={{
                  background: 'var(--color-accent-muted)',
                  color: 'var(--color-accent-600)',
                }}
              >
                {skillCommand.command}
              </span>
              {onClearSkillCommand ? (
                <Button unstyled
                  type="button"
                  aria-label={t('composer.removeSkill', { name: skillCommand.label })}
                  onClick={onClearSkillCommand}
                  className="ml-0.5 shrink-0 rounded-md p-0.5 transition-colors hover:bg-[var(--color-accent-muted)]"
                  style={{ color: 'var(--color-accent-700)' }}
                >
                  <X className="h-3 w-3" />
                </Button>
              ) : null}
            </div>
          </div>
        ) : null}
        <EditorContent
          editor={editor}
          className={`min-h-[40px] w-full text-sm text-foreground [&_.ProseMirror_a]:text-primary [&_.ProseMirror_a]:underline [&_.ProseMirror_a]:underline-offset-2 [&_.ProseMirror_a]:cursor-pointer [&_.ProseMirror_strong]:font-semibold [&_.ProseMirror_em]:italic [&_.ProseMirror_code]:rounded-md [&_.ProseMirror_code]:bg-muted [&_.ProseMirror_code]:px-1 [&_.ProseMirror_code]:text-[0.85em] [&_.ProseMirror_pre]:overflow-x-auto [&_.ProseMirror_pre]:rounded-md [&_.ProseMirror_pre]:bg-muted [&_.ProseMirror_pre]:p-2 [&_.ProseMirror_pre]:text-xs [&_.ProseMirror_pre_code]:bg-transparent [&_.ProseMirror_pre_code]:p-0 [&_.ProseMirror_ul]:list-disc [&_.ProseMirror_ul]:pl-5 [&_.ProseMirror_ol]:list-decimal [&_.ProseMirror_ol]:pl-5 [&_.ProseMirror_blockquote]:border-l-2 [&_.ProseMirror_blockquote]:border-border [&_.ProseMirror_blockquote]:pl-3 [&_.ProseMirror_blockquote]:opacity-90${limitEditorHeight ? ' [&_.ProseMirror]:max-h-[200px] [&_.ProseMirror]:overflow-y-auto [&_.ProseMirror]:overscroll-contain' : ''}`}
        />
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-0">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              data-aijia-composer-plus
              aria-label={t('composer.addAttachment')}
              onClick={onOpenAttachment}
              disabled={disabled}
              icon={<Plus />}
            />
            <Button
              type="button"
              variant={skillCommand ? 'secondary' : 'ghost'}
              size="sm"
              data-aijia-skill-picker-trigger
              onClick={onOpenSkill}
              disabled={disabled}
              aria-label={
                skillCommand
                  ? t('composer.openSkillPickerWithSkill', { name: skillCommand.label })
                  : t('composer.openSkillPicker')
              }
              aria-pressed={Boolean(skillCommand)}
              style={
                skillCommand
                  ? { background: 'var(--color-accent-subtle)', color: 'var(--color-accent-700)' }
                  : undefined
              }
              icon={<Blocks />}
            >
              {skillCommand ? t('composer.skillLoaded') : t('composer.skill')}
            </Button>
            {onPermissionModeChange ? (
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    disabled={disabled}
                    className="focus-visible:ring-0 data-[state=open]:bg-muted/70"
                    aria-label={t('composer.permissionModeLabel', {
                      mode: fullAccess
                        ? t('composer.permissionModeFull')
                        : t('composer.permissionModeDefault'),
                    })}
                    icon={fullAccess ? <ShieldCheck /> : <Shield />}
                  >
                    {fullAccess ? t('composer.permissionModeFull') : t('composer.permissionModeDefault')}
                    <ChevronDown className="h-3.5 w-3.5" aria-hidden="true" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent
                  side="top"
                  align="start"
                  sideOffset={8}
                  className="w-[288px] p-1.5"
                >
                  <DropdownMenuItem
                    className="flex h-9 cursor-pointer items-center gap-2 rounded-md px-2.5 text-sm"
                    onSelect={() => onPermissionModeChange('default')}
                  >
                    <span className="flex h-4 w-4 items-center justify-center text-foreground">
                      {!fullAccess ? <Check className="h-4 w-4" aria-hidden="true" /> : null}
                    </span>
                    <Shield className="h-4 w-4 text-muted-foreground" aria-hidden="true" />
                    <span className="truncate">{t('composer.permissionModeDefaultLong')}</span>
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    className="flex h-9 cursor-pointer items-center gap-2 rounded-md px-2.5 text-sm"
                    onSelect={() => onPermissionModeChange('fullAccess')}
                  >
                    <span className="flex h-4 w-4 items-center justify-center text-foreground">
                      {fullAccess ? <Check className="h-4 w-4" aria-hidden="true" /> : null}
                    </span>
                    <AlertTriangle className="h-4 w-4 text-muted-foreground" aria-hidden="true" />
                    <span className="truncate">{t('composer.permissionModeFullLong')}</span>
                  </DropdownMenuItem>
                  <DropdownMenuSeparator className="mx-1 my-1.5 bg-border" />
                  <div className="px-2.5 py-2 text-xs leading-5 text-muted-foreground">
                    <div className="mb-1 font-semibold text-foreground">
                      {t('composer.fullAccessRulesTitle')}
                    </div>
                    <p className="mb-1">{t('composer.fullAccessRulesIntro')}</p>
                    <ul className="list-disc space-y-1 pl-4">
                      <li>{t('composer.fullAccessRuleLessConfirm')}</li>
                      <li>{t('composer.fullAccessRuleSensitive')}</li>
                      <li>{t('composer.fullAccessRuleTrusted')}</li>
                      <li>{t('composer.fullAccessRuleSwitchBack')}</li>
                    </ul>
                  </div>
                </DropdownMenuContent>
              </DropdownMenu>
            ) : null}
            {showProjectButton ? (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={onPickProject}
                disabled={disabled}
                icon={<Folder />}
              >
                {projectLabel}
              </Button>
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
              <Button
                type="button"
                size="sm"
                aria-label={t('composer.stop')}
                onClick={() => onStop?.()}
                icon={stopIcon}
              />
            ) : (
              <Button
                type="button"
                size="md"
                aria-label={t('composer.send')}
                onClick={() => {
                  void trySubmit()
                }}
                disabled={sendDisabled}
                variant={sendDisabled ? 'secondary' : 'default'}
                icon={<ArrowUp />}
              />
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
