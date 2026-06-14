/**
 * @designSource design.pen#47U5w (proj1/conv1..3 + proj2/convA..B)
 *
 * 按 project 分组渲染会话；项目折叠状态由本组件内部 state 管理。
 * 置顶会话由 AppSidebar 全局 pinned 区域统一渲染（在 tab 切换条之上），
 * 这里只负责非置顶的项目分桶视图。
 */
import { ChevronDown, ChevronUp } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { ConversationRow } from './ConversationRow'
import { ProjectAccordion } from './ProjectAccordion'
import { type SidebarRowStatus } from './SidebarRowStatusIndicator'
import { loadCollapsedProjects, saveCollapsedProjects } from './sidebarProjectPrefs'
import { Button } from '@/components/ui/button'

const CONVERSATION_LIMIT_PER_PROJECT = 6

export interface ConversationTreeItem {
  id: string
  title: string
  active?: boolean
  status?: SidebarRowStatus
  pinned?: boolean
}

export interface ConversationTreeProject {
  id: string
  name: string
  conversations: ConversationTreeItem[]
}

interface ConversationTreeProps {
  projects?: ConversationTreeProject[]
  onSelectConversation?: (conversationId: string) => void
  onRenameConversation?: (id: string) => void
  onArchiveConversation?: (id: string) => void
  onTogglePinConversation?: (id: string, nextPinned: boolean) => void
}

export function ConversationTree({
  projects = [],
  onSelectConversation = () => {},
  onRenameConversation,
  onArchiveConversation,
  onTogglePinConversation,
}: ConversationTreeProps) {
  const { t } = useTranslation()
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>(() => loadCollapsedProjects())
  const [expandedProjectIds, setExpandedProjectIds] = useState<Record<string, boolean>>({})

  if (projects.length === 0) {
    return (
      <div className="px-2 py-4 text-sm text-muted-foreground">{t('sidebar.noHistory')}</div>
    )
  }

  return (
    <div className="flex w-full min-w-0 flex-col gap-1">
      {projects.map((p) => {
        const showAll = expandedProjectIds[p.id] ?? false
        const visibleConversations = showAll
          ? p.conversations
          : p.conversations.slice(0, CONVERSATION_LIMIT_PER_PROJECT)
        const hiddenCount = Math.max(0, p.conversations.length - CONVERSATION_LIMIT_PER_PROJECT)

        return (
          <ProjectAccordion
            key={p.id}
            name={p.name}
            expanded={!collapsed[p.id]}
            onToggle={() =>
              setCollapsed((s) => {
                const next = { ...s, [p.id]: !s[p.id] }
                saveCollapsedProjects(next)
                return next
              })
            }
          >
            {visibleConversations.map((c) => (
              <ConversationRow
                key={c.id}
                id={c.id}
                title={c.title}
                active={c.active}
                status={c.status}
                pinned={c.pinned}
                onClick={() => onSelectConversation(c.id)}
                onRename={() => onRenameConversation?.(c.id)}
                onArchive={() => onArchiveConversation?.(c.id)}
                onTogglePin={() => onTogglePinConversation?.(c.id, !c.pinned)}
              />
            ))}
            {hiddenCount > 0 ? (
              <Button unstyled
                type="button"
                onClick={() =>
                  setExpandedProjectIds((s) => ({
                    ...s,
                    [p.id]: !showAll,
                  }))
                }
                className="ml-[32px] my-1 flex items-center gap-1 rounded-md text-left text-xs font-medium text-[#636363] transition-colors"
              >
                {showAll ? (
                  <ChevronUp className="h-3.5 w-3.5 shrink-0" />
                ) : (
                  <ChevronDown className="h-3.5 w-3.5 shrink-0" />
                )}
                <span>
                  {showAll
                    ? t('sidebar.collapseConversations')
                    : t('sidebar.showMoreConversations', { count: hiddenCount })}
                </span>
              </Button>
            ) : null}
          </ProjectAccordion>
        )
      })}
    </div>
  )
}
