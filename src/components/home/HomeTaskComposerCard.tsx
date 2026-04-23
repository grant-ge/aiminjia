/**
 * @designSource design.pen#uq6ga ChatComposerCompact (home page variant)
 *
 * Submitting creates a new conversation and sends the first message.
 * Routing to the chat page is handled automatically by useChat.sendUserMessage.
 */
import { useState } from 'react'

import { ChatComposerCompact } from '@/components/chat-scene/ChatComposerCompact'
import { useAuthorizedWorkspace } from '@/hooks/useAuthorizedWorkspace'
import { useChat } from '@/hooks/useChat'
import { useWorkspaceAuthorization } from '@/hooks/useWorkspaceAuthorization'

export function HomeTaskComposerCard() {
  const [value, setValue] = useState('')
  const { sendUserMessage } = useChat()
  const { selectAndAuthorizeDirectory } = useWorkspaceAuthorization()
  // null sessionId = global/pre-conversation workspace
  const { workspace } = useAuthorizedWorkspace(null)

  const handleSubmit = async (text: string) => {
    if (!text.trim()) return
    setValue('')
    await sendUserMessage(text)
  }

  return (
    <ChatComposerCompact
      value={value}
      onChange={setValue}
      onSubmit={(v) => void handleSubmit(v)}
      placeholder="描述你的任务，或输入 / 选择技能来开始..."
      onPickProject={() => void selectAndAuthorizeDirectory(workspace?.rootPath)}
      projectLabel={workspace?.displayName ?? 'Desktop'}
    />
  )
}
