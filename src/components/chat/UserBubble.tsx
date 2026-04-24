/**
 * UserBubble — user message with optional file attachments.
 * Based on visual-prototype-zh.html .user-bubble styles.
 * Right-aligned per standard chat UI convention.
 */
import type { Message } from '@/types/message'
import { Avatar } from '@/components/common/Avatar'
import { FileAttachmentChip } from './FileAttachmentChip'
import { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'

interface UserBubbleProps {
  message: Message
  onResend?: (text: string) => void
}

export function UserBubble({ message, onResend }: UserBubbleProps) {
  const { t } = useTranslation()
  const { content, sender } = message
  const [isEditing, setIsEditing] = useState(false)
  const [draft, setDraft] = useState(content.text ?? '')
  const hasFiles = content.files && content.files.length > 0

  // Display sender name: use sender.name if available, fallback to "我"
  const displayName = sender?.name || t('userBubble.me')
  const isLoggedIn = sender?.isLoggedIn ?? false
  const canEdit = Boolean(content.text?.trim())

  const handleResend = useCallback(() => {
    const text = draft.trim()
    if (!text) return
    onResend?.(text)
    setIsEditing(false)
  }, [draft, onResend])

  return (
    <div className="mb-7 animate-[fadeUp_0.3s_ease]">
      {/* Header: name + avatar (right-aligned) */}
      <div className="mb-2 flex items-center justify-end gap-2">
        <span
          className="text-sm font-semibold"
          style={{ color: 'var(--color-text-primary)' }}
        >
          {displayName}
        </span>
        <Avatar variant="user" isLoggedIn={isLoggedIn} />
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
        {content.text && !isEditing && (
          <div className="flex max-w-[88%] flex-col items-end gap-1.5">
            <div
              className="inline-block rounded-xl rounded-br-[4px] px-4 py-2.5 text-base leading-relaxed"
              style={{
                background: 'var(--color-bg-msg-user)',
                color: 'var(--color-text-primary)',
              }}
            >
              {content.text}
            </div>
            {canEdit && (
              <button
                className="text-xs"
                style={{ color: 'var(--color-text-muted)' }}
                onClick={() => setIsEditing(true)}
              >
                {t('userBubble.editResend', '编辑并重发')}
              </button>
            )}
          </div>
        )}

        {content.text && isEditing && (
          <div className="w-full max-w-[88%] rounded-xl border p-2.5" style={{ borderColor: 'var(--color-border)' }}>
            <textarea
              className="w-full resize-y rounded-md border px-2 py-1.5 text-sm"
              style={{ borderColor: 'var(--color-border)', minHeight: '72px' }}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
            />
            <div className="mt-2 flex justify-end gap-2">
              <button
                className="rounded-md px-2.5 py-1 text-xs"
                style={{ border: '1px solid var(--color-border)' }}
                onClick={() => {
                  setDraft(content.text ?? '')
                  setIsEditing(false)
                }}
              >
                {t('common.cancel', '取消')}
              </button>
              <button
                className="rounded-md px-2.5 py-1 text-xs"
                style={{
                  background: 'var(--color-accent)',
                  color: 'var(--color-text-inverse)',
                }}
                onClick={handleResend}
              >
                {t('userBubble.resend', '重发')}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
