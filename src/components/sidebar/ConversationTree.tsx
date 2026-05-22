/**
 * @designSource design.pen#47U5w (proj1/conv1..3 + proj2/convA..B)
 *
 * 按 project 分组渲染会话；项目折叠状态由本组件内部 state 管理。
 * 置顶会话由 AppSidebar 全局 pinned 区域统一渲染（在 tab 切换条之上），
 * 这里只负责非置顶的项目分桶视图。
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { ConversationRow } from './ConversationRow'
import { ProjectAccordion } from './ProjectAccordion'

export interface ConversationTreeItem {
  id: string
  title: string
  active?: boolean
  loading?: boolean
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
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({})

  if (projects.length === 0) {
    return (
      <div className="px-2 py-4 text-sm text-muted-foreground">{t('sidebar.noHistory')}</div>
    )
  }

  return (
    <div className="flex flex-col gap-1">
      {projects.map((p) => (
        <ProjectAccordion
          key={p.id}
          name={p.name}
          expanded={!collapsed[p.id]}
          onToggle={() => setCollapsed((s) => ({ ...s, [p.id]: !s[p.id] }))}
        >
          {p.conversations.map((c) => (
            <ConversationRow
              key={c.id}
              id={c.id}
              title={c.title}
              active={c.active}
              loading={c.loading}
              pinned={c.pinned}
              onClick={() => onSelectConversation(c.id)}
              onRename={() => onRenameConversation?.(c.id)}
              onArchive={() => onArchiveConversation?.(c.id)}
              onTogglePin={() => onTogglePinConversation?.(c.id, !c.pinned)}
            />
          ))}
        </ProjectAccordion>
      ))}
    </div>
  )
}
