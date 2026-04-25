/**
 * @designSource design.pen#uq6ga ChatComposerCompact (home page variant)
 *
 * Flow:
 * 1. On mount: load persisted workspace from homeStore, or fetch default folder.
 * 2. On project button click: open folder picker, update homeStore.
 * 3. On submit: create conversation → authorize workspace → send message.
 */
import { useEffect, useRef, useState } from 'react'

import { SkillPopover } from '@/components/chat/SkillPopover'
import { SlashCommandPopover } from '@/components/chat/SlashCommandPopover'
import { ChatComposerCompact } from '@/components/chat-scene/ChatComposerCompact'
import { useChat } from '@/hooks/useChat'
import { useSkillComposer } from '@/hooks/useSkillComposer'
import {
  authorizeLocalDirectory,
  createConversation,
  getDefaultFolder,
  pickLocalDirectory,
  type AuthorizedWorkspaceRef,
} from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'
import { useHomeStore } from '@/stores/homeStore'
import { useUiStore } from '@/stores/uiStore'

export function HomeTaskComposerCard() {
  const [value, setValue] = useState('')
  const [isSubmitting, setIsSubmitting] = useState(false)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const { sendUserMessage } = useChat()

  const { selectedWorkspace, setSelectedWorkspace } = useHomeStore()
  const [displayWorkspace, setDisplayWorkspace] = useState<AuthorizedWorkspaceRef | null>(
    selectedWorkspace,
  )

  const {
    showSkillPopover,
    setShowSkillPopover,
    slashMatch,
    slashOpen,
    handleSkillPick,
    handleSlashSelect,
    handleSlashClose,
  } = useSkillComposer({
    input: value,
    setInput: setValue,
    textareaRef,
  })

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

      // Authorize the selected (or default) workspace
      const workspacePath = displayWorkspace?.rootPath
      if (workspacePath) {
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
      await sendUserMessage(text)
    } finally {
      setIsSubmitting(false)
    }
  }

  return (
    <div className="relative">
      <div className="absolute bottom-full left-10 z-30 mb-3">
        <SkillPopover
          open={showSkillPopover}
          onPick={handleSkillPick}
          onClose={() => setShowSkillPopover(false)}
        />
      </div>

      {slashOpen && slashMatch ? (
        <SlashCommandPopover
          filterText={slashMatch.filter}
          onSelect={handleSlashSelect}
          onClose={handleSlashClose}
        />
      ) : null}

      <ChatComposerCompact
        value={value}
        onChange={setValue}
        onSubmit={(v) => void handleSubmit(v)}
        placeholder="描述你的任务，或输入 / 选择技能来开始..."
        onOpenSkill={() => setShowSkillPopover((prev) => !prev)}
        onPickProject={() => void handlePickProject()}
        projectLabel={displayWorkspace?.displayName ?? '默认项目'}
        textareaRef={textareaRef}
        submitDisabled={isSubmitting}
      />
    </div>
  )
}
