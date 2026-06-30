/**
 * @designSource design.pen#uq6ga RichComposer (home page variant)
 *
 * Flow:
 * 1. On mount: load persisted workspace from homeStore, or fetch default folder.
 * 2. On project button click: open folder picker, update homeStore.
 * 3. On submit: create conversation → authorize workspace → send message.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { BriefcaseBusiness, ChevronDown, Folder, FolderPlus, House, X } from 'lucide-react'

import { SkillPopover } from '@/components/chat/SkillPopover'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  RichComposer,
  pendingAttachmentsToTokens,
  useComposerAttachmentPaste,
  useComposerDropInbox,
  type RichComposerHandle,
  type RichComposerSubmitPayload,
} from '@/components/rich-composer'
import { useChat, type PendingFileInfo } from '@/hooks/useChat'
import { useChatAttachments } from '@/hooks/useChatAttachments'
import {
  authorizeLocalDirectory,
  createConversation,
  getDefaultFolder,
  pickLocalDirectory,
  type PermissionMode,
  type ReasoningMode,
  type AuthorizedWorkspaceRef,
} from '@/lib/tauri'
import { localizeSkill, localizedSkillName } from '@/lib/skillLocalization'
import { useChatStore } from '@/stores/chatStore'
import { useHomeStore } from '@/stores/homeStore'
import { useSettingsStore } from '@/stores/settingsStore'
import { useSkillStore } from '@/stores/skillStore'
import { DRAFT_PERMISSION_SESSION_ID, DRAFT_REASONING_SESSION_ID, useUiStore } from '@/stores/uiStore'
import { Button } from '@/components/ui/button'

export function HomeTaskComposerCard() {
  const { t, i18n } = useTranslation()
  const composerRef = useRef<RichComposerHandle>(null)
  const [isSubmitting, setIsSubmitting] = useState(false)
  const { sendUserMessage } = useChat()
  const { isPickingAttachments, pickAttachments } = useChatAttachments()

  useComposerDropInbox(composerRef)
  useComposerAttachmentPaste(composerRef)

  const { selectedWorkspace, recentWorkspaces, setSelectedWorkspace, removeRecentWorkspace } = useHomeStore()
  const [displayWorkspace, setDisplayWorkspace] = useState<AuthorizedWorkspaceRef | null>(
    selectedWorkspace,
  )
  const [showSkillPopover, setShowSkillPopover] = useState(false)
  const skills = useSkillStore((s) => s.skills)
  const getSkillById = useSkillStore((s) => s.getById)
  const defaultPermissionMode = useSettingsStore((s) => s.defaultPermissionMode ?? 'default')
  const permissionMode = useUiStore((s) =>
    s.permissionModesBySession[DRAFT_PERMISSION_SESSION_ID] ?? defaultPermissionMode,
  )
  const setPermissionModeForSession = useUiStore((s) => s.setPermissionModeForSession)
  const reasoningMode = useUiStore((s) =>
    s.reasoningModesBySession[DRAFT_REASONING_SESSION_ID] ?? 'auto',
  )
  const setReasoningModeForSession = useUiStore((s) => s.setReasoningModeForSession)
  // Snapshot of the installed skills as composer-friendly tokens. Drives the
  // slash-command input rule and chip rendering inside the editor (mirrors
  // ChatBottomArea — single source of truth for selected skills).
  const skillTokens = useMemo(
    () =>
      skills.map((skill) => ({
        id: skill.id,
        label: localizeSkill(skill, i18n.language).name,
        command: skill.triggerText || `/${skill.id}`,
      })),
    [skills, i18n.language],
  )

  // One-shot prefill text; consumed synchronously via lazy initializer so
  // RichComposer's useEditor receives it on its very first render.
  const [initialMarkdown] = useState<string | undefined>(() => {
    const prefill = useUiStore.getState().consumePrefillText()
    return prefill ?? undefined
  })

  // One-shot pending skill from SkillsPage (e.g. user clicked "use this skill"
  // and got routed to home). Insert as an inline editor token after mount, so
  // it shows up as a chip in the composer body — same as ChatBottomArea.
  const [pendingSkill] = useState(() => useUiStore.getState().consumePendingSkill())
  const pendingSkillInsertedRef = useRef(false)
  useEffect(() => {
    if (!pendingSkill) return
    if (pendingSkillInsertedRef.current) return
    pendingSkillInsertedRef.current = true
    const skill = getSkillById(pendingSkill.id)
    composerRef.current?.insertSkillToken({
      id: pendingSkill.id,
      label: pendingSkill.label || localizedSkillName(skill, pendingSkill.id, i18n.language),
      command: pendingSkill.trigger || skill?.triggerText || `/${pendingSkill.id}`,
    })
    composerRef.current?.focus()
    // Run once after first render — composerRef is stable, deps intentionally empty.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const handleSkillPick = useCallback((skillId: string) => {
    const skill = getSkillById(skillId)
    composerRef.current?.insertSkillToken({
      id: skillId,
      label: localizedSkillName(skill, skillId, i18n.language),
      command: skill?.triggerText || `/${skillId}`,
    })
    composerRef.current?.focus()
    setShowSkillPopover(false)
  }, [getSkillById, i18n.language])

  const handlePermissionModeChange = useCallback((mode: PermissionMode) => {
    setPermissionModeForSession(DRAFT_PERMISSION_SESSION_ID, mode)
  }, [setPermissionModeForSession])

  const handleReasoningModeChange = useCallback((mode: ReasoningMode) => {
    setReasoningModeForSession(DRAFT_REASONING_SESSION_ID, mode)
  }, [setReasoningModeForSession])

  // Load default folder if no workspace has been selected yet
  useEffect(() => {
    if (selectedWorkspace) {
      setDisplayWorkspace(selectedWorkspace)
      return
    }
    getDefaultFolder()
      .then((ws) => setDisplayWorkspace(ws))
      .catch(() => {
        // fallback: show nothing, user can pick manually
      })
  }, [selectedWorkspace])

  const selectWorkspace = useCallback((ws: AuthorizedWorkspaceRef) => {
    setSelectedWorkspace(ws)
    setDisplayWorkspace(ws)
  }, [setSelectedWorkspace])

  // Switch back to the implicit default folder. Because its id is 'default',
  // handleSubmit's `isDefaultFolder` branch (line ~135) skips workspace
  // authorize — restoring the fast home→chat path without the IPC await that
  // can race ChatPage's switchConversation effect.
  const handleSelectDefaultFolder = useCallback(async () => {
    try {
      const ws = await getDefaultFolder()
      selectWorkspace(ws)
    } catch {
      // fall back to clearing the selection — UI will show "默认项目"
      setSelectedWorkspace(null)
      setDisplayWorkspace(null)
    }
  }, [selectWorkspace, setSelectedWorkspace])

  const handlePickProject = async () => {
    const path = await pickLocalDirectory({
      defaultPath: displayWorkspace?.rootPath,
      title: t('homeComposer.selectWorkDir'),
    })
    if (!path) return
    const parts = path.split(/[/\\]/).filter(Boolean)
    const name = parts[parts.length - 1] ?? path
    selectWorkspace({ id: name, rootPath: path, displayName: name })
  }

  const handlePickAttachments = useCallback(async () => {
    const results = await pickAttachments()
    if (results.length > 0) {
      composerRef.current?.insertAttachmentTokens(pendingAttachmentsToTokens(results))
    }
  }, [pickAttachments])

  const handleSubmit = useCallback(async (payload: RichComposerSubmitPayload) => {
    if (isSubmitting) return
    setIsSubmitting(true)
    try {
      // Create conversation first so we have an ID to authorize against
      const backendId = await createConversation()
      setPermissionModeForSession(backendId, permissionMode)
      setReasoningModeForSession(backendId, reasoningMode)
      const now = new Date().toISOString()
      const store = useChatStore.getState()
      store.setConversations([
        { id: backendId, title: t('homeComposer.newConversation'), createdAt: now, updatedAt: now, isArchived: false },
        ...store.conversations,
      ])
      store.setMessages([])
      useUiStore.getState().setRoute({ kind: 'chat', conversationId: backendId })
      // Switch sidebar to 项目 tab so the new conversation is visible in the
      // sidebar list — same UX as the employee / expert-team paths.
      useUiStore.getState().setSidebarTab('project')

      // Authorize the selected workspace. Skip when it's the implicit default
      // folder (id === 'default') — leaving workspaceName empty lets the sidebar
      // fallback group it under "默认文件夹" instead of creating a duplicate
      // "defaultFolder" project from the path's last component.
      const workspacePath = displayWorkspace?.rootPath
      const isDefaultFolder = displayWorkspace?.id === 'default'
      if (workspacePath && !isDefaultFolder) {
        try {
          await authorizeLocalDirectory(workspacePath, backendId)
          // Patch workspaceName into the optimistic conversation so the sidebar
          // groups it correctly without waiting for a full getConversations reload.
          const ws = displayWorkspace
          if (ws?.displayName) {
            const s = useChatStore.getState()
            s.setConversations(
              s.conversations.map((c) =>
                c.id === backendId ? { ...c, workspaceName: ws.displayName } : c,
              ),
            )
          }
        } catch (err) {
          console.error('[HomeTaskComposerCard] Failed to authorize workspace:', err)
          // Non-fatal: proceed without workspace authorization
        }
      }

      const fileInfos: PendingFileInfo[] = payload.attachments.map((f) => ({
        id: f.id,
        fileName: f.fileName,
        filePath: f.path,
        kind: f.kind,
        fileSize: f.fileSize,
        fileType: f.fileType,
        mimeType: f.mimeType,
      }))
      // The inline skill chip is the source of truth — it travels with the
      // doc, gets cleared automatically on submit, and is collected by the
      // serializer into payload.skills (mirrors ChatBottomArea).
      const skillForThisTurn = payload.skills[0] ?? null
      await sendUserMessage(payload.markdown, fileInfos, skillForThisTurn, permissionMode, reasoningMode)
    } finally {
      setIsSubmitting(false)
    }
  }, [
    displayWorkspace,
    isSubmitting,
    permissionMode,
    reasoningMode,
    sendUserMessage,
    setPermissionModeForSession,
    setReasoningModeForSession,
    t,
  ])

  const workspaceLabel = displayWorkspace?.displayName ?? t('homeComposer.defaultProject')
  const workspacePath = displayWorkspace?.rootPath

  return (
    <div
      data-testid="home-composer-shell"
      className="home-composer-large relative isolate overflow-visible rounded-md shadow-[var(--shadow-card)] [&_[data-testid=composer-root]]:relative [&_[data-testid=composer-root]]:z-10 [&_[data-testid=composer-root]]:rounded-md [&_[data-testid=composer-root]]:border-border [&_[data-testid=composer-root]]:px-5 [&_[data-testid=composer-root]]:pb-4 [&_[data-testid=composer-root]]:pt-5 [&_[data-testid=composer-root]]:shadow-none [&_[data-testid=composer-root]>div:has(.ProseMirror)]:min-h-[60px] [&_[data-testid=composer-root]_.ProseMirror]:min-h-[60px]"
    >
      <div className="absolute bottom-full left-1/2 z-30 mb-1 -translate-x-1/2">
        <SkillPopover
          open={showSkillPopover}
          onPick={handleSkillPick}
          onClose={() => setShowSkillPopover(false)}
        />
      </div>

      <RichComposer
        ref={composerRef}
        placeholder={t('homeComposer.placeholder')}
        onSubmit={handleSubmit}
        disabled={isSubmitting}
        clearOnSubmit
        autoFocus
        initialMarkdown={initialMarkdown}
        onOpenSkill={() => setShowSkillPopover((prev) => !prev)}
        skillTokens={skillTokens}
        permissionMode={permissionMode}
        onPermissionModeChange={handlePermissionModeChange}
        reasoningMode={reasoningMode}
        onReasoningModeChange={handleReasoningModeChange}
        showProjectButton={false}
        limitEditorHeight
        onOpenAttachment={isPickingAttachments ? undefined : () => void handlePickAttachments()}
      />

      <div
        data-testid="home-workspace-bar"
        className="absolute inset-x-0 top-full z-0 flex min-h-[58px] -translate-y-2 items-center justify-between rounded-b-md border-x border-b border-border bg-sidebar px-5 pt-2"
      >
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button unstyled
              type="button"
              data-aijia-workspace-trigger
              disabled={isSubmitting}
              aria-label={t('homeComposer.selectWorkDirAria', { name: workspaceLabel })}
              title={workspacePath}
              className="inline-flex max-w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:opacity-40"
            >
              <BriefcaseBusiness className="h-5 w-5 shrink-0" />
              <span className="truncate">{t('homeComposer.workingIn', { name: workspaceLabel })}</span>
              <ChevronDown className="h-4 w-4 shrink-0 text-muted-foreground" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            align="start"
            side="bottom"
            sideOffset={8}
            className="w-[300px] max-w-[calc(100vw-32px)] rounded-md border-border bg-card p-1 shadow-[var(--shadow-popover)]"
          >
            {recentWorkspaces.filter((ws) => ws.id !== 'default').length > 0 ? (
              <div className="max-h-[200px] overflow-y-auto">
                {recentWorkspaces.filter((ws) => ws.id !== 'default').map((ws) => (
                  <DropdownMenuItem
                    key={ws.rootPath}
                    data-aijia-workspace-recent
                    data-aijia-workspace-path={ws.rootPath}
                    onSelect={() => selectWorkspace(ws)}
                    title={ws.rootPath}
                    className="group flex cursor-pointer items-center gap-3 rounded-md px-3 py-2 text-sm font-medium outline-none focus:bg-muted"
                  >
                    <Folder className="h-4 w-4 shrink-0 text-muted-foreground" />
                    <span className="flex-1 truncate text-foreground">{ws.displayName}</span>
                    <Button unstyled
                      type="button"
                      aria-label={`从最近列表中移除 ${ws.displayName}`}
                      onClick={(e) => {
                        e.preventDefault()
                        e.stopPropagation()
                        removeRecentWorkspace(ws.rootPath)
                      }}
                      onPointerDown={(e) => e.stopPropagation()}
                      className="hidden h-5 w-5 shrink-0 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-[rgba(var(--muted-foreground-rgb),0.10)] hover:text-foreground group-hover:flex group-data-[highlighted]:flex"
                    >
                      <X className="h-3.5 w-3.5" />
                    </Button>
                  </DropdownMenuItem>
                ))}
              </div>
            ) : null}
            {recentWorkspaces.filter((ws) => ws.id !== 'default').length > 0 ? <DropdownMenuSeparator className="mx-2 bg-border" /> : null}
            <DropdownMenuItem
              data-aijia-workspace-action="pick-default"
              onSelect={() => void handleSelectDefaultFolder()}
              className="flex cursor-pointer items-center gap-3 rounded-md px-3 py-2 text-sm font-medium outline-none focus:bg-muted"
            >
              <House className="h-4 w-4 shrink-0 text-muted-foreground" />
              <span>{t('homeComposer.useDefaultFolder')}</span>
            </DropdownMenuItem>
            <DropdownMenuItem
              data-aijia-workspace-action="pick-other"
              onSelect={() => void handlePickProject()}
              className="flex cursor-pointer items-center gap-3 rounded-md px-3 py-2 text-sm font-medium outline-none focus:bg-muted"
            >
              <FolderPlus className="h-4 w-4 shrink-0 text-muted-foreground" />
              <span>{t('homeComposer.selectOtherDir')}</span>
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </div>
  )
}
