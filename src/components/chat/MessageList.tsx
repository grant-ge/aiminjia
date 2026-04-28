/**
 * @designSource design.pen#F8ixG flow
 * @sizing padding [24,40] gap 18
 */
import { AiBubble } from '@/components/chat/AiBubble'
import { StreamingBubble } from '@/components/chat/StreamingBubble'
import { GeneratedFileCard } from '@/components/chat-scene/GeneratedFileCard'
import { SuggestChipGroup } from '@/components/chat-scene/SuggestChipGroup'
import { ToolGroupCard } from '@/components/chat-scene/ToolGroupCard'
import { UserMessageBubble } from '@/components/chat-scene/UserMessageBubble'
import { openGeneratedFile } from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'
import { useTurnRenderModel } from '@/hooks/useTurnRenderModel'

export function MessageList() {
  const turns = useTurnRenderModel()
  const isStreaming = useChatStore((s) => s.isStreaming)
  const streamingContent = useChatStore((s) => {
    const activeId = s.activeConversationId
    return activeId ? (s.streamStates[activeId]?.streamingContent ?? '') : ''
  })
  return (
    <div className="flex flex-col gap-5 py-3">
      {turns.map((t, i) => {
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
                appName={f.appName}
                onOpen={() => void openGeneratedFile(f.id, f.conversationId)}
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
