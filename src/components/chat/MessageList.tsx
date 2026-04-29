/**
 * @designSource design.pen#F8ixG flow
 * @sizing padding [24,40] gap 18
 */
import { useEffect } from 'react'

import { AiBubble } from '@/components/chat/AiBubble'
import { StreamingBubble } from '@/components/chat/StreamingBubble'
import { GeneratedFileCard } from '@/components/chat-scene/GeneratedFileCard'
import { SuggestChipGroup } from '@/components/chat-scene/SuggestChipGroup'
import { ToolGroupCard } from '@/components/chat-scene/ToolGroupCard'
import { UserMessageBubble } from '@/components/chat-scene/UserMessageBubble'
import { toPreviewTarget } from '@/components/chat/generatedFileActions'
import { useChatStore } from '@/stores/chatStore'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { useChat } from '@/hooks/useChat'
import { useTurnRenderModel, type RenderGeneratedFile } from '@/hooks/useTurnRenderModel'
import { openGeneratedFile, revealFileInFolder } from '@/lib/tauri'

type FileActionKind = 'preview' | 'open' | 'reveal'

const FILE_ACTION_ERROR_TITLES: Record<FileActionKind, string> = {
  preview: '无法预览文件',
  open: '无法打开文件',
  reveal: '无法定位文件',
}

export function MessageList() {
  const turns = useTurnRenderModel()
  useChat()
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const isStreaming = useChatStore((s) => s.isStreaming)
  const streamingContent = useChatStore((s) => {
    const activeId = s.activeConversationId
    return activeId ? (s.streamStates[activeId]?.streamingContent ?? '') : ''
  })
  const openPreview = useGeneratedFilePreviewStore((s) => s.openPreview)
  const clearIfConversationChanged = useGeneratedFilePreviewStore((s) => s.clearIfConversationChanged)
  const pushNotification = useNotificationStore((s) => s.push)

  useEffect(() => {
    if (activeConversationId) clearIfConversationChanged(activeConversationId)
  }, [activeConversationId, clearIfConversationChanged])

  const notifyFileError = (kind: FileActionKind, message: string) => {
    pushNotification({
      level: 'error',
      title: FILE_ACTION_ERROR_TITLES[kind],
      message,
      actions: [],
      dismissible: true,
      context: 'toast',
    })
  }

  const handlePreview = (file: RenderGeneratedFile) => {
    if (!file.conversationId) {
      notifyFileError('preview', '生成文件缺少所属对话，无法预览。')
      return
    }
    openPreview(toPreviewTarget(file, file.conversationId))
  }

  const handleOpenExternal = async (file: RenderGeneratedFile) => {
    if (!file.conversationId) {
      notifyFileError('open', '生成文件缺少所属对话，无法打开。')
      return
    }
    try {
      await openGeneratedFile(file.id, file.conversationId)
    } catch (err) {
      notifyFileError('open', err instanceof Error ? err.message : '打开生成文件失败。')
    }
  }

  const handleReveal = async (file: RenderGeneratedFile) => {
    if (!file.conversationId) {
      notifyFileError('reveal', '生成文件缺少所属对话，无法定位。')
      return
    }
    try {
      await revealFileInFolder(file.id, file.conversationId)
    } catch (err) {
      notifyFileError('reveal', err instanceof Error ? err.message : '定位生成文件失败。')
    }
  }

  return (
    <div className="flex flex-col gap-5 px-2 py-3">
      {turns.map((t, i) => {
        return (
          <div key={i} className="flex flex-col gap-4">
            {t.userMessage ? (
              <UserMessageBubble
                text={t.userMessage.text}
                commandText={t.userMessage.commandText}
                skillCommand={t.userMessage.skillCommand}
                files={t.userMessage.files}
              />
            ) : null}
            {t.toolGroup ? (
              <ToolGroupCard
                status={t.toolGroup.status}
                steps={t.toolGroup.steps}
              />
            ) : null}
            {t.aiSegments.map((s) => (
              <AiBubble key={s.id} message={s.message} />
            ))}
            {t.generatedFiles.map((f) => (
              <GeneratedFileCard
                key={f.id}
                title={f.title}
                sub={f.sub}
                appName={f.primaryAction === 'preview' ? '预览' : f.appName}
                primaryAction={f.primaryAction}
                canPreview={f.canPreview}
                canOpenExternal={f.canOpenExternal}
                canReveal={f.canReveal}
                onPreview={() => handlePreview(f)}
                onOpenExternal={() => void handleOpenExternal(f)}
                onReveal={() => void handleReveal(f)}
              />
            ))}
            {t.suggestions.length > 0 ? (
              <SuggestChipGroup
                items={t.suggestions.map((s) => ({ label: s, onClick: () => {} }))}
              />
            ) : null}
          </div>
        )
      })}
      {isStreaming ? <StreamingBubble content={streamingContent} /> : null}
    </div>
  )
}
