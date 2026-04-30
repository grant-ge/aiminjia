/**
 * @designSource design.pen#uq6ga ChatComposerCompact (home page variant)
 *
 * Flow:
 * 1. On mount: load persisted workspace from homeStore, or fetch default folder.
 * 2. On project button click: open folder picker, update homeStore.
 * 3. On submit: create conversation → authorize workspace → send message.
 */
import { useCallback, useEffect, useRef, useState } from 'react'

import { SkillPopover } from '@/components/chat/SkillPopover'
import { PendingAttachmentChips } from '@/components/chat/PendingAttachmentChips'
import { ChatComposerCompact } from '@/components/chat-scene/ChatComposerCompact'
import { useChat, type PendingFileInfo } from '@/hooks/useChat'
import { type PendingAttachment } from '@/hooks/useChatAttachments'
import { useComposerPaste } from '@/hooks/useComposerPaste'
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
  const [value, setValue] = useState('')
  const [isSubmitting, setIsSubmitting] = useState(false)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const { sendUserMessage } = useChat()

  const [pendingFiles, setPendingFiles] = useState<PendingAttachment[]>([])

  const appendPendingFiles = useCallback((resolved: PendingAttachment[]) => {
    setPendingFiles((prev) => {
      const seen = new Set(prev.map((file) => file.id))
      const next = resolved.filter((file) => !seen.has(file.id))
      return next.length > 0 ? [...prev, ...next] : prev
    })
  }, [])

  const { handlePaste } = useComposerPaste({ onAttachmentsResolved: appendPendingFiles })

  const { selectedWorkspace, setSelectedWorkspace } = useHomeStore()
  const [displayWorkspace, setDisplayWorkspace] = useState<AuthorizedWorkspaceRef | null>(
    selectedWorkspace,
  )
  const [showSkillPopover, setShowSkillPopover] = useState(false)
  const getSkillById = useSkillStore((s) => s.getById)

  useEffect(() => {
    const prefill = useUiStore.getState().consumePrefillText()
    if (prefill) {
      setValue(prefill)
      requestAnimationFrame(() => {
        const el = textareaRef.current
        if (!el) return
        el.focus()
        el.setSelectionRange(prefill.length, prefill.length)
      })
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const handleSkillPick = useCallback((skillId: string) => {
    const skill = getSkillById(skillId)
    const trigger = skill?.triggerText || `/${skillId}`
    const next = trigger.endsWith(' ') ? trigger : `${trigger} `
    setValue(next)
    setShowSkillPopover(false)
    requestAnimationFrame(() => {
      const el = textareaRef.current
      if (!el) return
      el.focus()
      el.setSelectionRange(next.length, next.length)
    })
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

  const handleSubmit = async (text: string) => {
    if (!text.trim() || isSubmitting) return
    setIsSubmitting(true)
    try {
      // Create conversation first so we have an ID to authorize against
      const backendId = await createConversation()
      setValue('')
      const now = new Date().toISOString()
      const store = useChatStore.getState()
      store.setConversations([
        { id: backendId, title: '新对话', createdAt: now, updatedAt: now, isArchived: false },
        ...store.conversations,
      ])
      store.setActiveConversation(backendId)
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

      // sendUserMessage will use the already-active conversation
      const fileInfos: PendingFileInfo[] = pendingFiles.map((f) => ({
        id: f.id,
        fileName: f.fileName,
        filePath: f.path,
        kind: f.kind,
        fileSize: f.fileSize,
        fileType: f.fileType,
        mimeType: f.mimeType,
      }))
      await sendUserMessage(text, fileInfos)
      setPendingFiles([])
    } finally {
      setIsSubmitting(false)
    }
  }

  return (
    <div className="relative">
      <div className="absolute top-full left-1/2 z-30 mt-1 -translate-x-1/2">
        <SkillPopover
          open={showSkillPopover}
          onPick={handleSkillPick}
          onClose={() => setShowSkillPopover(false)}
        />
      </div>

      <ChatComposerCompact
        value={value}
        onChange={setValue}
        onSubmit={(v) => void handleSubmit(v)}
        placeholder="描述你的任务，或点击「技能」按钮选择技能..."
        onOpenSkill={() => setShowSkillPopover((prev) => !prev)}
        onPickProject={() => void handlePickProject()}
        projectLabel={displayWorkspace?.displayName ?? '默认项目'}
        textareaRef={textareaRef}
        submitDisabled={isSubmitting}
        onPaste={handlePaste}
        pendingFilesSlot={pendingFiles.length > 0 ? (
          <PendingAttachmentChips
            pendingFiles={pendingFiles}
            onRemove={(id: string) => setPendingFiles((prev) => prev.filter((f) => f.id !== id))}
          />
        ) : null}
      />
    </div>
  )
}
