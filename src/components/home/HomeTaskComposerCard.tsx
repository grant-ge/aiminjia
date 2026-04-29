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
import { SlashCommandPopover } from '@/components/chat/SlashCommandPopover'
import { ChatComposerCompact } from '@/components/chat-scene/ChatComposerCompact'
import { useChat, type PendingFileInfo } from '@/hooks/useChat'
import { type PendingAttachment } from '@/hooks/useChatAttachments'
import { useComposerPaste } from '@/hooks/useComposerPaste'
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

const FILE_TYPE_CONFIG: Record<string, { label: string; bg: string; color: string }> = {
  excel: { label: 'XLS', bg: 'var(--color-filetype-green-bg)', color: 'var(--color-semantic-green)' },
  csv: { label: 'CSV', bg: 'var(--color-filetype-green-bg)', color: 'var(--color-semantic-green)' },
  word: { label: 'DOC', bg: 'var(--color-filetype-blue-bg)', color: 'var(--color-semantic-blue)' },
  pdf: { label: 'PDF', bg: 'var(--color-filetype-red-bg)', color: 'var(--color-semantic-red)' },
  json: { label: 'JSON', bg: 'var(--color-filetype-accent-bg)', color: 'var(--color-accent)' },
  image: { label: 'IMG', bg: 'var(--color-filetype-blue-bg)', color: 'var(--color-semantic-blue)' },
  folder: { label: 'DIR', bg: 'var(--color-primary-subtle)', color: 'var(--color-text-primary)' },
}

function PendingFiles({
  pendingFiles,
  onRemove,
}: {
  pendingFiles: PendingAttachment[]
  onRemove: (id: string) => void
}) {
  return (
    <div className="flex flex-wrap gap-2">
      {pendingFiles.map((file) => {
        const config = FILE_TYPE_CONFIG[file.fileType] ?? FILE_TYPE_CONFIG.csv
        return (
          <div
            key={file.id}
            className="inline-flex items-center gap-2 rounded-lg py-1.5 pr-2 pl-2.5"
            style={{ background: config.bg }}
          >
            <span className="text-xs font-bold" style={{ color: config.color }}>
              {config.label}
            </span>
            <span className="max-w-[180px] truncate text-xs font-medium" style={{ color: 'var(--color-text-primary)' }}>
              {file.fileName}
            </span>
            <button
              type="button"
              className="flex h-4 w-4 shrink-0 items-center justify-center rounded-full border-none transition-colors"
              style={{
                background: 'var(--color-primary-subtle)',
                color: 'var(--color-text-muted)',
              }}
              onClick={() => onRemove(file.id)}
            >
              <svg className="h-2.5 w-2.5" viewBox="0 0 24 24" fill="currentColor">
                <path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z" />
              </svg>
            </button>
          </div>
        )
      })}
    </div>
  )
}

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
        onPaste={handlePaste}
        pendingFilesSlot={pendingFiles.length > 0 ? (
          <PendingFiles
            pendingFiles={pendingFiles}
            onRemove={(id) => setPendingFiles((prev) => prev.filter((f) => f.id !== id))}
          />
        ) : null}
      />
    </div>
  )
}
