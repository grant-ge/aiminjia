/**
 * Sidebar — Chat history list, new chat button, settings button.
 * Includes persona switcher at the top.
 */
import { useEffect, useMemo, useState, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { useChat } from '@/hooks/useChat'
import { useChatStore } from '@/stores/chatStore'
import { useAuthStore } from '@/stores/authStore'
import { useSettingsStore } from '@/stores/settingsStore'
import { usePersonaStore } from '@/stores/personaStore'
import { useBrandingStore } from '@/stores/brandingStore'
import { useProductName } from '@/hooks/useProductName'
import { updateSettings, getSettings, exportConversation } from '@/lib/tauri'
import { useNotificationStore } from '@/stores/notificationStore'
import type { Conversation } from '@/types/message'

interface SidebarProps {
  onOpenSettings: () => void
}

type TimeGroup = 'today' | 'yesterday' | 'thisWeek' | 'earlier'

function getTimeGroup(dateStr: string): TimeGroup {
  const date = new Date(dateStr)
  const now = new Date()
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate())
  const yesterday = new Date(today)
  yesterday.setDate(yesterday.getDate() - 1)
  const weekStart = new Date(today)
  weekStart.setDate(weekStart.getDate() - today.getDay())

  if (date >= today) return 'today'
  if (date >= yesterday) return 'yesterday'
  if (date >= weekStart) return 'thisWeek'
  return 'earlier'
}

function groupConversations(
  conversations: Conversation[],
): { group: TimeGroup; items: Conversation[] }[] {
  const order: TimeGroup[] = ['today', 'yesterday', 'thisWeek', 'earlier']
  const groups = new Map<TimeGroup, Conversation[]>()

  for (const conv of conversations) {
    const group = getTimeGroup(conv.updatedAt)
    if (!groups.has(group)) groups.set(group, [])
    groups.get(group)!.push(conv)
  }

  return order.filter((g) => groups.has(g)).map((g) => ({ group: g, items: groups.get(g)! }))
}

function highlightMatch(text: string, query: string) {
  const trimmed = query.trim()
  if (!trimmed) return text

  const lowerText = text.toLowerCase()
  const lowerQuery = trimmed.toLowerCase()
  const matchIndex = lowerText.indexOf(lowerQuery)
  if (matchIndex === -1) return text

  const matchEnd = matchIndex + trimmed.length
  return (
    <>
      {text.slice(0, matchIndex)}
      <mark
        style={{
          background: 'var(--color-accent-light)',
          color: 'inherit',
          borderRadius: '2px',
          padding: '0 1px',
        }}
      >
        {text.slice(matchIndex, matchEnd)}
      </mark>
      {text.slice(matchEnd)}
    </>
  )
}

export function Sidebar({ onOpenSettings }: SidebarProps) {
  const { t } = useTranslation()
  const {
    conversations,
    activeConversationId,
    createNewConversation,
    switchConversation,
    deleteConversation,
    renameConversation,
  } = useChat()

  const busyConversations = useChatStore((s) => s.busyConversations)
  const isNewDisabled = busyConversations.size >= 3
  const isLoggedIn = useAuthStore((s) => s.isLoggedIn)
  const authUser = useAuthStore((s) => s.user)
  const authTenant = useAuthStore((s) => s.tenant)
  const productName = useProductName()
  const logoUrl = useBrandingStore((s) => s.logoUrl)
  const accentColor = useBrandingStore((s) => s.accentColor)
  const useCloud = useSettingsStore((s) => s.useCloud)

  const { personas, activePersona, setActive: setActivePersona } = usePersonaStore()
  const [personaListOpen, setPersonaListOpen] = useState(false)

  const [editingId, setEditingId] = useState<string | null>(null)
  const [editTitle, setEditTitle] = useState('')
  const [searchQuery, setSearchQuery] = useState('')
  const [menuOpenId, setMenuOpenId] = useState<string | null>(null)
  const editInputRef = useRef<HTMLInputElement>(null)
  const menuRef = useRef<HTMLDivElement | null>(null)

  const filteredConversations = useMemo(() => {
    const query = searchQuery.trim().toLowerCase()
    if (!query) return conversations
    return conversations.filter((conversation) =>
      conversation.title.toLowerCase().includes(query),
    )
  }, [conversations, searchQuery])

  const grouped = useMemo(
    () => groupConversations(filteredConversations),
    [filteredConversations],
  )

  const [appVersion, setAppVersion] = useState('...')
  useEffect(() => {
    import('@tauri-apps/api/app').then(({ getVersion }) =>
      getVersion().then(setAppVersion)
    ).catch(() => setAppVersion('0.0.0'))
  }, [])

  useEffect(() => {
    if (!menuOpenId) return

    const closeMenu = (event: MouseEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) {
        setMenuOpenId(null)
      }
    }

    document.addEventListener('mousedown', closeMenu)
    return () => document.removeEventListener('mousedown', closeMenu)
  }, [menuOpenId])

  const handleExport = async (conversationId: string, format: 'html' | 'pdf') => {
    setMenuOpenId(null)
    try {
      const result = await exportConversation(conversationId, format)
      useNotificationStore.getState().push({
        level: 'success',
        title: t('topBar.exportSuccess'),
        message: result.fileName,
        actions: [],
        dismissible: true,
        autoHide: 5,
        context: 'toast',
      })
    } catch (err) {
      useNotificationStore.getState().push({
        level: 'error',
        title: t('topBar.exportFailed'),
        message: String(err),
        actions: [],
        dismissible: true,
        autoHide: 8,
        context: 'toast',
      })
    }
  }

  return (
    <aside
      className="flex w-[260px] shrink-0 flex-col border-r"
      style={{
        background: 'var(--color-bg-sidebar)',
        borderColor: 'var(--color-border)',
      }}
    >
      {/* Header — same bg as sidebar, logo icon uses accent */}
      <div
        className="border-b px-4 pt-4 pb-3"
        style={{ borderColor: 'var(--color-border)' }}
      >
        <div className="flex items-center gap-2.5">
          <img
            src={logoUrl}
            alt={productName}
            className="h-6 w-6 rounded"
            onError={(e) => {
              if (e.currentTarget.src !== window.location.origin + '/app-icon.png') {
                e.currentTarget.src = '/app-icon.png'
              }
            }}
          />
          <span
            className="text-lg font-bold"
            style={{ color: 'var(--color-text-primary)' }}
          >
            {productName}
          </span>
        </div>

        {/* Persona switcher — accent subtle bg + accent text */}
        <div className="mt-2">
          <button
            type="button"
            className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left transition-colors cursor-pointer"
            style={{
              background: personaListOpen ? 'var(--color-accent-muted)' : 'var(--color-accent-subtle)',
              color: 'var(--color-accent-700)',
              border: '1px solid var(--color-accent-border)',
            }}
            onMouseEnter={(e) => {
              e.currentTarget.style.background = 'var(--color-accent-muted)'
            }}
            onMouseLeave={(e) => {
              if (!personaListOpen) {
                e.currentTarget.style.background = 'var(--color-accent-subtle)'
              }
            }}
            onClick={() => setPersonaListOpen(!personaListOpen)}
          >
            <span className="text-base leading-none">{activePersona?.icon || '👤'}</span>
            <span className="flex-1 truncate text-sm font-semibold">
              {activePersona ? t(`personas.${activePersona.id}`, activePersona.name) : t('sidebar.selectPersona')}
            </span>
            <span
              className="text-xs transition-transform"
              style={{
                color: 'var(--color-accent-600)',
                transform: personaListOpen ? 'rotate(180deg)' : 'rotate(0deg)',
              }}
            >
              ▾
            </span>
          </button>

          {personaListOpen && (
            <div className="mt-1 space-y-0.5 rounded-md py-1"
              style={{ background: 'var(--color-bg-sidebar-hover)' }}
            >
              {personas.map((p) => (
                <button
                  key={p.id}
                  type="button"
                  className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm transition-colors cursor-pointer"
                  style={{
                    background: p.id === activePersona?.id ? 'var(--color-primary-subtle)' : 'transparent',
                    color: p.id === activePersona?.id ? 'var(--color-primary)' : 'var(--color-text-secondary)',
                  }}
                  onMouseEnter={(e) => {
                    if (p.id !== activePersona?.id) {
                      e.currentTarget.style.background = 'var(--color-bg-sidebar-hover)'
                    }
                  }}
                  onMouseLeave={(e) => {
                    if (p.id !== activePersona?.id) {
                      e.currentTarget.style.background = 'transparent'
                    }
                  }}
                  onClick={async () => {
                    try {
                      await setActivePersona(p.id)
                      setPersonaListOpen(false)
                    } catch (err) {
                      console.error('Failed to switch persona:', err)
                    }
                  }}
                >
                  <span className="text-base leading-none">{p.icon}</span>
                  <span className="flex-1 truncate">{t(`personas.${p.id}`, p.name)}</span>
                  {p.id === activePersona?.id && (
                    <span style={{ color: 'var(--color-primary)' }}>✓</span>
                  )}
                </button>
              ))}
            </div>
          )}
        </div>

        <button
          className={`mt-3 flex h-9 w-full items-center justify-center gap-2 rounded-md border px-3.5 text-sm font-medium transition-all duration-150 ${isNewDisabled ? 'cursor-not-allowed opacity-50' : 'cursor-pointer'}`}
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
          title={isNewDisabled ? t('sidebar.maxConcurrent') : ''}
          onClick={() => !isNewDisabled && createNewConversation()}
        >
          <svg
            className="h-4 w-4 shrink-0"
            viewBox="0 0 24 24"
            fill="currentColor"
          >
            <path d="M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6v2z" />
          </svg>
          {t('sidebar.newChat')}
        </button>

        <div className="relative mt-2">
          <input
            type="search"
            className="w-full rounded-md border py-1.5 pl-8 pr-3 text-sm outline-none transition-colors"
            style={{
              background: 'var(--color-bg-main)',
              borderColor: 'var(--color-border)',
              color: 'var(--color-text-primary)',
            }}
            placeholder={t('sidebar.searchPlaceholder')}
            value={searchQuery}
            onChange={(event) => setSearchQuery(event.target.value)}
          />
          <svg
            className="pointer-events-none absolute top-1/2 left-2 h-3.5 w-3.5 -translate-y-1/2 opacity-40"
            viewBox="0 0 24 24"
            fill="currentColor"
            style={{ color: 'var(--color-text-muted)' }}
          >
            <path d="M15.5 14h-.79l-.28-.27A6.471 6.471 0 0 0 16 9.5 6.5 6.5 0 1 0 9.5 16c1.61 0 3.09-.59 4.23-1.57l.27.28v.79l5 4.99L20.49 19l-4.99-5zm-6 0C7.01 14 5 11.99 5 9.5S7.01 5 9.5 5 14 7.01 14 9.5 11.99 14 9.5 14z" />
          </svg>
        </div>
      </div>

      {/* Chat history list */}
      <nav className="flex-1 overflow-x-hidden overflow-y-auto p-2">
        {conversations.length === 0 ? (
          <p
            className="px-3 py-8 text-center text-sm"
            style={{ color: 'var(--color-text-muted)' }}
          >
            {t('sidebar.noConversations')}
          </p>
        ) : filteredConversations.length === 0 && searchQuery.trim() ? (
          <p
            className="px-3 py-8 text-center text-sm"
            style={{ color: 'var(--color-text-muted)' }}
          >
            {t('sidebar.noSearchResults')}
          </p>
        ) : (
          grouped.map(({ group, items }) => (
            <div key={group} className="mb-1">
              <div
                className="px-3 pt-2 pb-1 text-xs font-medium"
                style={{ color: 'var(--color-text-muted)' }}
              >
                {t('sidebar.timeGroup.' + group)}
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
                  {/* Active indicator: accent-colored left bar */}
                  {conv.id === activeConversationId && (
                    <span
                      className="absolute top-2 bottom-2 left-0 w-[3px] rounded"
                      style={{ background: accentColor }}
                    />
                  )}
                  <button
                    className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 border-none bg-transparent px-3 py-2 text-left"
                    onClick={() => switchConversation(conv.id)}
                    onDoubleClick={(e) => {
                      e.preventDefault()
                      setEditingId(conv.id)
                      setEditTitle(conv.title)
                      setTimeout(() => editInputRef.current?.select(), 0)
                    }}
                  >
                    {busyConversations.has(conv.id) ? (
                      <span className="relative flex h-[18px] w-[18px] shrink-0 items-center justify-center">
                        <span
                          className="absolute h-[14px] w-[14px] animate-ping rounded-full opacity-40"
                          style={{ background: accentColor }}
                        />
                        <span
                          className="relative h-[8px] w-[8px] rounded-full"
                          style={{ background: accentColor }}
                        />
                      </span>
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
                    {editingId === conv.id ? (
                      <input
                        ref={editInputRef}
                        className="flex-1 truncate rounded border bg-transparent px-1 text-sm outline-none"
                        style={{
                          color: 'var(--color-text-primary)',
                          borderColor: 'var(--color-primary)',
                        }}
                        value={editTitle}
                        onChange={(e) => setEditTitle(e.target.value)}
                        onBlur={() => {
                          const trimmed = editTitle.trim()
                          if (trimmed && trimmed !== conv.title) {
                            renameConversation(conv.id, trimmed)
                          }
                          setEditingId(null)
                        }}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') {
                            e.currentTarget.blur()
                          } else if (e.key === 'Escape') {
                            setEditingId(null)
                          }
                        }}
                        onClick={(e) => e.stopPropagation()}
                        onDoubleClick={(e) => e.stopPropagation()}
                      />
                    ) : (
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
                      {highlightMatch(conv.title, searchQuery)}
                    </span>
                    )}
                  </button>
                  <div
                    className="relative mr-2 shrink-0 opacity-0 transition-opacity duration-150 group-hover:opacity-100"
                    ref={menuOpenId === conv.id ? menuRef : null}
                  >
                    {/*
                      Keep each actions trigger uniquely named so screen readers and tests
                      can target the intended conversation even when multiple rows are visible.
                    */}
                    <button
                      className="flex h-6 w-6 cursor-pointer items-center justify-center rounded border-none"
                      style={{
                        background: 'transparent',
                        color: 'var(--color-text-muted)',
                      }}
                      aria-label={`${t('sidebar.conversationActions')} ${conv.title}`}
                      title={`${t('sidebar.conversationActions')} ${conv.title}`}
                      onClick={(e) => {
                        e.stopPropagation()
                        setMenuOpenId((current) => current === conv.id ? null : conv.id)
                      }}
                    >
                      <svg
                        className="h-4 w-4"
                        viewBox="0 0 24 24"
                        fill="currentColor"
                      >
                        <path d="M12 7a2 2 0 110-4 2 2 0 010 4zm0 7a2 2 0 110-4 2 2 0 010 4zm0 7a2 2 0 110-4 2 2 0 010 4z" />
                      </svg>
                    </button>

                    {menuOpenId === conv.id && (
                      <div
                        className="absolute right-0 top-full z-10 mt-1 min-w-[160px] overflow-hidden rounded-lg border py-1"
                        style={{
                          background: 'var(--color-bg-card)',
                          borderColor: 'var(--color-border)',
                          boxShadow: 'var(--shadow-modal)',
                        }}
                      >
                        <button
                          className="flex w-full cursor-pointer items-center gap-2 border-none px-3 py-2 text-sm transition-colors"
                          style={{
                            background: 'transparent',
                            color: 'var(--color-text-secondary)',
                          }}
                          onClick={(e) => {
                            e.stopPropagation()
                            void handleExport(conv.id, 'html')
                          }}
                        >
                          {t('topBar.exportAsHtml')}
                        </button>
                        <button
                          className="flex w-full cursor-pointer items-center gap-2 border-none px-3 py-2 text-sm transition-colors"
                          style={{
                            background: 'transparent',
                            color: 'var(--color-text-secondary)',
                          }}
                          onClick={(e) => {
                            e.stopPropagation()
                            void handleExport(conv.id, 'pdf')
                          }}
                        >
                          {t('topBar.exportAsPdf')}
                        </button>
                        <button
                          className="flex w-full cursor-pointer items-center gap-2 border-none px-3 py-2 text-sm transition-colors"
                          style={{
                            background: 'transparent',
                            color: 'var(--color-semantic-red)',
                          }}
                          onClick={(e) => {
                            e.stopPropagation()
                            setMenuOpenId(null)
                            deleteConversation(conv.id)
                          }}
                        >
                          {t('sidebar.deleteConversation')}
                        </button>
                      </div>
                    )}
                  </div>
                </div>
              ))}
            </div>
          ))
        )}
      </nav>

      {/* Footer */}
      <div
        className="border-t px-4 py-3"
        style={{
          borderColor: 'var(--color-border)',
          fontSize: 'var(--text-xs)',
          color: 'var(--color-text-muted)',
        }}
      >
        {isLoggedIn && (
          <div className="mb-2 flex items-center gap-2">
            <div
              className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full text-xs font-semibold"
              style={{
                background: 'var(--color-primary-subtle)',
                color: 'var(--color-primary)',
              }}
            >
              {(authUser?.name ?? authUser?.username ?? '?')[0].toUpperCase()}
            </div>
            <div className="min-w-0 flex-1">
              <div
                className="truncate text-xs font-medium"
                style={{ color: 'var(--color-text-primary)' }}
              >
                {authUser?.name ?? authUser?.username}
              </div>
              {authTenant?.balance && (
                <div className="truncate text-[10px]" style={{ color: 'var(--color-text-muted)' }}>
                  {authTenant.name} · {authTenant.balance}
                </div>
              )}
            </div>
            <button
              className="shrink-0 rounded-md px-2 py-0.5 text-[10px] font-medium transition-colors"
              style={{
                background: useCloud ? 'var(--color-primary-subtle)' : 'var(--color-bg-main)',
                color: useCloud ? 'var(--color-primary)' : 'var(--color-text-muted)',
                border: '1px solid',
                borderColor: useCloud ? 'var(--color-primary)' : 'var(--color-border)',
                cursor: 'pointer',
              }}
              title={useCloud ? t('sidebar.switchToLocal') : t('sidebar.switchToCloud')}
              onClick={async () => {
                try {
                  const s = await getSettings()
                  await updateSettings({ ...s, useCloud: !useCloud })
                  useSettingsStore.getState().setSettings({ useCloud: !useCloud })
                } catch (err) {
                  console.error('Failed to toggle useCloud:', err)
                }
              }}
            >
              {useCloud ? t('sidebar.cloud') : t('sidebar.local')}
            </button>
          </div>
        )}
        <div className="flex items-center justify-between">
        <span>{productName} v{appVersion}</span>
        <button
          className="flex h-9 cursor-pointer items-center gap-1.5 rounded-md border px-3.5 text-sm font-medium transition-all duration-150"
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
          {t('sidebar.settings')}
        </button>
        </div>
      </div>
    </aside>
  )
}
