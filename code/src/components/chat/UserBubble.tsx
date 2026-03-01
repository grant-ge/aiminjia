/**
 * UserBubble — user message with optional file attachments.
 * Based on visual-prototype-zh.html .user-bubble styles.
 * Right-aligned per standard chat UI convention.
 */
import type { Message } from '@/types/message'
import { Avatar } from '@/components/common/Avatar'
import { FileAttachmentChip } from './FileAttachmentChip'

interface UserBubbleProps {
  message: Message
}

export function UserBubble({ message }: UserBubbleProps) {
  const { content } = message
  const hasFiles = content.files && content.files.length > 0

  return (
    <div className="mb-7 animate-[fadeUp_0.3s_ease]">
      {/* Header: name + avatar (right-aligned) */}
      <div className="mb-2 flex items-center justify-end gap-2">
        <span
          className="text-sm font-semibold"
          style={{ color: 'var(--color-text-primary)' }}
        >
          我
        </span>
        <Avatar variant="user" />
      </div>

      {/* Body — offset by avatar width, right-aligned */}
      <div className="flex flex-col items-end pr-9">
        {/* File attachments */}
        {hasFiles && (
          <div className="mb-1.5 flex flex-col items-end gap-1">
            {content.files!.map((file) => (
              <FileAttachmentChip key={file.id} file={file} />
            ))}
          </div>
        )}

        {/* Text bubble */}
        {content.text && (
          <div
            className="inline-block max-w-[88%] rounded-xl rounded-br-[4px] px-4 py-2.5 text-base leading-relaxed"
            style={{
              background: 'var(--color-bg-msg-user)',
              color: 'var(--color-text-primary)',
            }}
          >
            {content.text}
          </div>
        )}
      </div>
    </div>
  )
}
