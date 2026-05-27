/**
 * resendLastMessage — utility used by the streaming:error toast's
 * "重发" / "Resend" button.
 *
 * When a network-class error knocks the stream offline (`chunk_timeout`,
 * 5xx, connection reset, etc.) we leave the user's original message in
 * the chat history — see useStreaming.ts:371-375 for the historical
 * reason why we DON'T delete it on error. That means a resend is just
 * "re-send the IPC with the same args"; no message duplication, no new
 * optimistic bubble.
 *
 * This lives outside the React tree so it can be wired into a
 * Notification `action` callback (toasts have no React context).
 */
import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { recordDiagnostic } from '@/lib/diagnostics'
import i18n from '@/i18n'
import { sendMessage, type ChatAttachmentPayload, type SkillCommandPayload } from '@/lib/tauri'
import type { Message } from '@/types/message'

/**
 * Picks the most recent user message in the named conversation. We walk
 * messages from the end because that's far cheaper than scanning the full
 * (potentially thousands-deep) history when the latest turn just failed.
 */
function findLastUserMessage(conversationId: string): Message | undefined {
  const { messages } = useChatStore.getState()
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i]
    if (m.conversationId === conversationId && m.role === 'user') {
      return m
    }
  }
  return undefined
}

/**
 * Convert a stored FileAttachment back into the `ChatAttachmentPayload`
 * shape `send_message` expects on the Tauri side.
 *
 * `filePath` is **required** by the backend's attachment derivation logic
 * (`path_auth::derive_working_dirs_from_attachments`); attachments without
 * a usable path are dropped — uploading them again from raw bytes would
 * require re-running the file-picker flow, which is out of scope for a
 * silent retry.
 */
function fileAttachmentsToPayload(message: Message): ChatAttachmentPayload[] {
  const files = message.content.files
  if (!files || files.length === 0) return []
  return files
    .filter((f) => !!f.filePath)
    .map((f) => ({
      id: f.id,
      fileName: f.fileName,
      filePath: f.filePath!,
      kind: f.kind ?? 'file',
      fileSize: f.fileSize,
      fileType: f.fileType,
      mimeType: f.mimeType,
    }))
}

function skillCommandToPayload(message: Message): SkillCommandPayload | null {
  const sc = message.content.skillCommand
  if (!sc) return null
  return { id: sc.id, label: sc.label, command: sc.command }
}

/**
 * Re-send the most recent user message in this conversation by replaying
 * the same `send_message` IPC. Surfaces a toast if there's nothing to
 * resend (rare — would mean the conversation never had a user message
 * yet), and a separate toast if the IPC itself fails.
 *
 * Reuses the original message's `clientMessageId` so the backend's
 * dedup-on-id path treats this as a retry, not a fresh send.
 */
export async function resendLastUserMessage(conversationId: string): Promise<void> {
  const message = findLastUserMessage(conversationId)
  if (!message) {
    useNotificationStore.getState().push({
      level: 'warning',
      title: i18n.t('errors.resendNoUserMessage', '没有可重发的消息'),
      message: '',
      actions: [],
      dismissible: true,
      autoHide: 5,
      context: 'toast',
    })
    return
  }

  const text = message.content.text ?? ''
  const attachments = fileAttachmentsToPayload(message)
  const skillCommand = skillCommandToPayload(message)

  recordDiagnostic({
    event: 'chat.resend.triggered',
    conversationId,
    clientMessageId: message.id,
    payload: {
      textLength: text.length,
      attachmentCount: attachments.length,
      hasSkillCommand: skillCommand !== null,
    },
  })

  // Mark the conversation as streaming again so the UI immediately shows
  // the loading state — otherwise there's a 100-200ms gap where the toast
  // is dismissed but the message looks finished. The backend will keep
  // this in sync via the next streaming:delta / streaming:done.
  useChatStore.getState().setConversationStreaming(conversationId, true)
  useChatStore.getState().addBusyConversation(conversationId)

  try {
    await sendMessage(
      conversationId,
      text,
      attachments,
      null,
      message.id,
      skillCommand,
    )
  } catch (err) {
    console.error('[resendLastUserMessage] sendMessage IPC failed:', err)
    // Roll back the busy flag so the user can try again.
    useChatStore.getState().setConversationStreaming(conversationId, false)
    useChatStore.getState().removeBusyConversation(conversationId)
    useNotificationStore.getState().push({
      level: 'error',
      title: i18n.t('errors.resendFailed', '重发失败'),
      message: String(err),
      actions: [],
      dismissible: true,
      autoHide: 8,
      context: 'toast',
    })
  }
}
