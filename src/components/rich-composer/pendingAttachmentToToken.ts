import type { ComposerAttachmentToken } from './types'
import type { PendingAttachment } from '@/hooks/useChatAttachments'

export function pendingAttachmentToToken(
  attachment: PendingAttachment,
): ComposerAttachmentToken {
  return {
    id: attachment.id,
    fileName: attachment.fileName,
    path: attachment.path,
    kind: attachment.kind,
    fileType: attachment.fileType,
    fileSize: attachment.fileSize,
    mimeType: attachment.mimeType,
    source: attachment.source,
  }
}

export function pendingAttachmentsToTokens(
  attachments: PendingAttachment[],
): ComposerAttachmentToken[] {
  return attachments.map(pendingAttachmentToToken)
}
