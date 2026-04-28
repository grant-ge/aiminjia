/**
 * @designSource design.pen#F8ixG flow
 * @sizing padding [24,40] gap 18
 */
import { useState } from 'react'
import { FileSpreadsheet } from 'lucide-react'

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
  const { sendUserMessage } = useChat()
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const isStreaming = useChatStore((s) => s.isStreaming)
  const streamingContent = useChatStore((s) => {
    const activeId = s.activeConversationId
    return activeId ? (s.streamStates[activeId]?.streamingContent ?? '') : ''
  })
  const openPreview = useGeneratedFilePreviewStore((s) => s.openPreview)
  const pushNotification = useNotificationStore((s) => s.push)
  const [expansion, setExpansion] = useState<
    Record<number, { expanded: boolean; stepIndex: number | null }>
  >({})

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
    if (!activeConversationId) {
      notifyFileError('preview', '当前没有可用对话，无法预览生成文件。')
      return
    }
    openPreview(toPreviewTarget(file, activeConversationId))
  }

  const handleOpenExternal = async (file: RenderGeneratedFile) => {
    if (!activeConversationId) {
      notifyFileError('open', '当前没有可用对话，无法打开生成文件。')
      return
    }
    try {
      await openGeneratedFile(file.id, activeConversationId)
    } catch (err) {
      notifyFileError('open', err instanceof Error ? err.message : '打开生成文件失败。')
    }
  }

  const handleReveal = async (file: RenderGeneratedFile) => {
    if (!activeConversationId) {
      notifyFileError('reveal', '当前没有可用对话，无法定位生成文件。')
      return
    }
    try {
      await revealFileInFolder(file.id, activeConversationId)
    } catch (err) {
      notifyFileError('reveal', err instanceof Error ? err.message : '定位生成文件失败。')
    }
  }

  return (
    <div className="flex flex-col gap-5 px-6 py-3">
      {turns.map((t, i) => {
        const e = expansion[i] ?? { expanded: true, stepIndex: null }
        return (
          <div key={i} className="flex flex-col gap-4">
            {t.userMessage ? (
              <UserMessageBubble
                text={t.userMessage.text}
                commandText={t.userMessage.commandText}
                skillCommand={t.userMessage.skillCommand}
              />
            ) : null}
            {t.toolGroup ? (
              <ToolGroupCard
                status={t.toolGroup.status}
                steps={t.toolGroup.steps}
                durationMs={t.toolGroup.durationMs}
                expanded={e.expanded}
                expandedStepIndex={e.stepIndex}
                onToggle={() =>
                  setExpansion((prev) => ({ ...prev, [i]: { ...e, expanded: !e.expanded } }))
                }
                onToggleStep={(index) =>
                  setExpansion((prev) => ({
                    ...prev,
                    [i]: { ...e, stepIndex: e.stepIndex === index ? null : index },
                  }))
                }
              />
            ) : null}
            {t.aiSegments.map((s) => (
              <AiBubble key={s.id} message={s.message} onUserResponse={sendUserMessage} />
            ))}
            {t.generatedFiles.map((f) => (
              <GeneratedFileCard
                key={f.id}
                title={f.title}
                sub={f.sub}
                appName={f.primaryAction === 'preview' ? 'Preview' : f.appName}
                fileIcon={<FileSpreadsheet className="h-4 w-4 text-muted-foreground" />}
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
