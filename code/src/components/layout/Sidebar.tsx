/**
 * Sidebar — Chat history list, new chat button, settings button.
 * Based on visual-prototype-zh.html sidebar section.
 */
import { useMemo } from 'react'
import { useChat } from '@/hooks/useChat'
import { useChatStore } from '@/stores/chatStore'
import type { Conversation } from '@/types/message'

interface SidebarProps {
  onOpenSettings: () => void
}

type TimeGroup = '今天' | '昨天' | '本周' | '更早'

function getTimeGroup(dateStr: string): TimeGroup {
  const date = new Date(dateStr)
  const now = new Date()
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate())
  const yesterday = new Date(today)
  yesterday.setDate(yesterday.getDate() - 1)
  const weekStart = new Date(today)
  weekStart.setDate(weekStart.getDate() - today.getDay())

  if (date >= today) return '今天'
  if (date >= yesterday) return '昨天'
  if (date >= weekStart) return '本周'
  return '更早'
}

function groupConversations(
  conversations: Conversation[],
): { group: TimeGroup; items: Conversation[] }[] {
  const order: TimeGroup[] = ['今天', '昨天', '本周', '更早']
  const groups = new Map<TimeGroup, Conversation[]>()

  for (const conv of conversations) {
    const group = getTimeGroup(conv.updatedAt)
    if (!groups.has(group)) groups.set(group, [])
    groups.get(group)!.push(conv)
  }

  return order.filter((g) => groups.has(g)).map((g) => ({ group: g, items: groups.get(g)! }))
}

export function Sidebar({ onOpenSettings }: SidebarProps) {
  const {
    conversations,
    activeConversationId,
    createNewConversation,
    switchConversation,
    deleteConversation,
  } = useChat()

  const busyConversations = useChatStore((s) => s.busyConversations)
  const isNewDisabled = busyConversations.size >= 3

  const grouped = useMemo(() => groupConversations(conversations), [conversations])

  return (
    <aside
      className="flex w-[260px] shrink-0 flex-col border-r"
      style={{
        background: 'var(--color-bg-sidebar)',
        borderColor: 'var(--color-border)',
      }}
    >
      {/* Header */}
      <div
        className="border-b px-4 pt-4 pb-3"
        style={{ borderColor: 'var(--color-border)' }}
      >
        <div className="flex items-center gap-2.5">
          <img
            src="/renlijia.png"
            alt="AI小家"
            className="h-6 w-6 rounded"
          />
          <span
            className="text-lg font-bold"
            style={{ color: 'var(--color-text-primary)' }}
          >
            AI小家
          </span>
        </div>
        <p
          className="mt-1 text-xs"
          style={{ color: 'var(--color-text-muted)' }}
        >
          组织专家
        </p>

        <button
          className={`mt-3 flex w-full items-center justify-center gap-2 rounded-sm border px-[18px] py-2 text-base font-medium transition-all duration-150 ${isNewDisabled ? 'cursor-not-allowed opacity-50' : 'cursor-pointer'}`}
          style={{
            borderColor: 'var(--color-primary)',
            color: 'var(--color-primary)',
            background: 'transparent',
          }}
          onMouseEnter={(e) => {
            if (!isNewDisabled) {
              e.currentTarget.style.background = 'var(--color-primary-subtle)'
            }
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background = 'transparent'
          }}
          disabled={isNewDisabled}
          title={isNewDisabled ? '已达最大并发数，请等待' : ''}
          onClick={() => !isNewDisabled && createNewConversation()}
        >
          <svg
            className="h-4 w-4 shrink-0"
            viewBox="0 0 24 24"
            fill="currentColor"
          >
            <path d="M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6v2z" />
          </svg>
          新对话
        </button>
      </div>

      {/* Chat history list */}
      <nav className="flex-1 overflow-x-hidden overflow-y-auto p-2">
        {conversations.length === 0 ? (
          <p
            className="px-3 py-8 text-center text-sm"
            style={{ color: 'var(--color-text-muted)' }}
          >
            暂无对话记录
          </p>
        ) : (
          grouped.map(({ group, items }) => (
            <div key={group} className="mb-1">
              <div
                className="px-3 pt-2 pb-1 text-xs font-medium"
                style={{ color: 'var(--color-text-muted)' }}
              >
                {group}
              </div>
              {items.map((conv) => (
                <div
                  key={conv.id}
                  className="group relative mb-0.5 flex w-full items-center rounded-md transition-all duration-150"
                  style={{
                    background:
                      conv.id === activeConversationId
                        ? 'var(--color-bg-sidebar-hover)'
                        : 'transparent',
                  }}
                  onMouseEnter={(e) => {
                    if (conv.id !== activeConversationId) {
                      e.currentTarget.style.background =
                        'var(--color-bg-sidebar-hover)'
                    }
                  }}
                  onMouseLeave={(e) => {
                    if (conv.id !== activeConversationId) {
                      e.currentTarget.style.background = 'transparent'
                    }
                  }}
                >
                  {conv.id === activeConversationId && (
                    <span
                      className="absolute top-2 bottom-2 left-0 w-[3px] rounded"
                      style={{ background: 'var(--color-primary)' }}
                    />
                  )}
                  <button
                    className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 border-none bg-transparent px-3 py-2 text-left"
                    onClick={() => switchConversation(conv.id)}
                  >
                    {busyConversations.has(conv.id) ? (
                      <span
                        className="h-[18px] w-[18px] shrink-0 animate-spin rounded-full border-2 border-current border-t-transparent opacity-60"
                        style={{ color: 'var(--color-text-muted)' }}
                      />
                    ) : (
                      <svg
                        className="h-[18px] w-[18px] shrink-0 opacity-60"
                        viewBox="0 0 24 24"
                        fill="currentColor"
                        style={{ color: 'var(--color-text-muted)' }}
                      >
                        <path d="M20 2H4c-1.1 0-2 .9-2 2v18l4-4h14c1.1 0 2-.9 2-2V4c0-1.1-.9-2-2-2z" />
                      </svg>
                    )}
                    <span
                      className="flex-1 truncate text-sm"
                      style={{
                        color:
                          conv.id === activeConversationId
                            ? 'var(--color-text-primary)'
                            : 'var(--color-text-secondary)',
                        fontWeight: conv.id === activeConversationId ? 500 : 400,
                      }}
                    >
                      {conv.title}
                    </span>
                  </button>
                  <button
                    className="mr-2 flex h-6 w-6 shrink-0 cursor-pointer items-center justify-center rounded border-none opacity-0 transition-opacity duration-150 group-hover:opacity-100"
                    style={{
                      background: 'transparent',
                      color: 'var(--color-text-muted)',
                    }}
                    title="删除对话"
                    onClick={(e) => {
                      e.stopPropagation()
                      deleteConversation(conv.id)
                    }}
                    onMouseEnter={(e) => {
                      e.currentTarget.style.color = 'var(--color-semantic-red)'
                    }}
                    onMouseLeave={(e) => {
                      e.currentTarget.style.color = 'var(--color-text-muted)'
                    }}
                  >
                    <svg
                      className="h-3.5 w-3.5"
                      viewBox="0 0 24 24"
                      fill="currentColor"
                    >
                      <path d="M6 19c0 1.1.9 2 2 2h8c1.1 0 2-.9 2-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z" />
                    </svg>
                  </button>
                </div>
              ))}
            </div>
          ))
        )}
      </nav>

      {/* Footer */}
      <div
        className="flex items-center justify-between border-t px-4 py-3"
        style={{
          borderColor: 'var(--color-border)',
          fontSize: 'var(--text-xs)',
          color: 'var(--color-text-muted)',
        }}
      >
        <span>AI小家 v0.1.0</span>
        <button
          className="flex cursor-pointer items-center gap-1.5 rounded-sm border px-[18px] py-2 text-base font-medium transition-all duration-150"
          style={{
            borderColor: 'var(--color-border)',
            color: 'var(--color-text-muted)',
            background: 'transparent',
          }}
          onClick={onOpenSettings}
          onMouseEnter={(e) => {
            e.currentTarget.style.background = 'var(--color-bg-sidebar-hover)'
            e.currentTarget.style.color = 'var(--color-text-secondary)'
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background = 'transparent'
            e.currentTarget.style.color = 'var(--color-text-muted)'
          }}
        >
          <svg
            className="h-4 w-4"
            viewBox="0 0 24 24"
            fill="currentColor"
          >
            <path d="M19.14 12.94c.04-.3.06-.61.06-.94 0-.32-.02-.64-.07-.94l2.03-1.58a.49.49 0 00.12-.61l-1.92-3.32a.49.49 0 00-.59-.22l-2.39.96c-.5-.38-1.03-.7-1.62-.94l-.36-2.54a.484.484 0 00-.48-.41h-3.84c-.24 0-.43.17-.47.41l-.36 2.54c-.59.24-1.13.57-1.62.94l-2.39-.96c-.22-.08-.47 0-.59.22L2.74 8.87c-.12.21-.08.47.12.61l2.03 1.58c-.05.3-.07.62-.07.94s.02.64.07.94l-2.03 1.58a.49.49 0 00-.12.61l1.92 3.32c.12.22.37.29.59.22l2.39-.96c.5.38 1.03.7 1.62.94l.36 2.54c.05.24.24.41.48.41h3.84c.24 0 .44-.17.47-.41l.36-2.54c.59-.24 1.13-.56 1.62-.94l2.39.96c.22.08.47 0 .59-.22l1.92-3.32c.12-.22.07-.47-.12-.61l-2.01-1.58zM12 15.6c-1.98 0-3.6-1.62-3.6-3.6s1.62-3.6 3.6-3.6 3.6 1.62 3.6 3.6-1.62 3.6-3.6 3.6z" />
          </svg>
          设置
        </button>
      </div>
    </aside>
  )
}
