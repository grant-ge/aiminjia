/**
 * @designSource design.pen#uq6ga RichComposer (home page variant)
 *
 * Flow:
 * 1. On mount: load persisted workspace from homeStore, or fetch default folder.
 * 2. On project button click: open folder picker, update homeStore.
 * 3. On submit: create conversation → authorize workspace → send message.
 */
import { useCallback, useEffect, useRef, useState } from 'react'

import { SkillPopover } from '@/components/chat/SkillPopover'
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
  type AuthorizedWorkspaceRef,
} from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'
import { useHomeStore } from '@/stores/homeStore'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

export function HomeTaskComposerCard() {
  const composerRef = useRef<RichComposerHandle>(null)
  const [isSubmitting, setIsSubmitting] = useState(false)
  const { sendUserMessage } = useChat()
  const { isPickingAttachments, pickAttachments } = useChatAttachments()

  useComposerDropInbox(composerRef)
  useComposerAttachmentPaste(composerRef)

  const { selectedWorkspace, setSelectedWorkspace } = useHomeStore()
  const [displayWorkspace, setDisplayWorkspace] = useState<AuthorizedWorkspaceRef | null>(
    selectedWorkspace,
  )
  const [showSkillPopover, setShowSkillPopover] = useState(false)
  const getSkillById = useSkillStore((s) => s.getById)

  // One-shot prefill text; consumed synchronously via lazy initializer so
  // RichComposer's useEditor receives it on its very first render.
  const [initialMarkdown] = useState<string | undefined>(() => {
    const prefill = useUiStore.getState().consumePrefillText()
    return prefill ?? undefined
  })

  const handleSkillPick = useCallback((skillId: string) => {
    const skill = getSkillById(skillId)
    const trigger = skill?.triggerText || `/${skillId}`
    const next = trigger.endsWith(' ') ? trigger : `${trigger} `
    composerRef.current?.clear()
    composerRef.current?.getEditor()?.commands.insertContent(next)
    composerRef.current?.focus()
    setShowSkillPopover(false)
  }, [getSkillById])

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

  const handlePickProject = async () => {
    const path = await pickLocalDirectory({
      defaultPath: displayWorkspace?.rootPath,
      title: '选择工作目录',
    })
    if (!path) return
    const parts = path.split(/[/\\]/).filter(Boolean)
    const name = parts[parts.length - 1] ?? path
    const ws: AuthorizedWorkspaceRef = { id: name, rootPath: path, displayName: name }
    setSelectedWorkspace(ws)
    setDisplayWorkspace(ws)
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
      const now = new Date().toISOString()
      const store = useChatStore.getState()
      store.setConversations([
        { id: backendId, title: '新对话', createdAt: now, updatedAt: now, isArchived: false },
        ...store.conversations,
      ])
      store.setMessages([])
      useUiStore.getState().setRoute({ kind: 'chat', conversationId: backendId })

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
      await sendUserMessage(payload.markdown, fileInfos)
    } finally {
      setIsSubmitting(false)
    }
  }, [displayWorkspace, isSubmitting, sendUserMessage])

  return (
    <div className="relative">
      <div className="absolute bottom-full left-1/2 z-30 mb-1 -translate-x-1/2">
        <SkillPopover
          open={showSkillPopover}
          onPick={handleSkillPick}
          onClose={() => setShowSkillPopover(false)}
        />
      </div>

      <RichComposer
        ref={composerRef}
        placeholder="描述你的任务，或点击「技能」按钮选择技能..."
        onSubmit={handleSubmit}
        disabled={isSubmitting}
        clearOnSubmit
        autoFocus
        initialMarkdown={initialMarkdown}
        onOpenSkill={() => setShowSkillPopover((prev) => !prev)}
        onPickProject={() => void handlePickProject()}
        projectLabel={displayWorkspace?.displayName ?? '默认项目'}
        onOpenAttachment={isPickingAttachments ? undefined : () => void handlePickAttachments()}
      />
    </div>
  )
}
