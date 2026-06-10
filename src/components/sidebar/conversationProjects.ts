/**
 * 把 useChat() 的 flat conversations 转成 project-grouped 结构。
 *
 * 分组依据：conversation.workspaceName（后端从 authorized_workspace_store 注入）。
 * 未绑定工作目录的对话归到"默认文件夹"。
 *
 * 置顶会话不在这里特殊处理 —— AppSidebar 把所有 pinned 会话提到全局"置顶"区
 * （tab 切换条之上），传进来的 conversations 列表已经过 pinned 过滤。
 */
import i18n from '@/i18n'

import type { ConversationTreeProject } from './ConversationTree'
import type { SidebarRowStatus } from './SidebarRowStatusIndicator'

export interface RawConversation {
  id: string
  title: string
  workspaceName?: string | null
  status?: SidebarRowStatus
  isPinned?: boolean
}

const DEFAULT_PROJECT_ID = 'default'
function getDefaultProjectName() { return i18n.t('sidebar.defaultFolder') }

export function groupConversationsByProject(
  conversations: RawConversation[],
  activeId: string | null,
): ConversationTreeProject[] {
  const map = new Map<string, ConversationTreeProject>()
  for (const c of conversations) {
    const projectId = c.workspaceName ?? DEFAULT_PROJECT_ID
    const projectName = c.workspaceName ?? getDefaultProjectName()
    let project = map.get(projectId)
    if (!project) {
      project = { id: projectId, name: projectName, conversations: [] }
      map.set(projectId, project)
    }
    project.conversations.push({
      id: c.id,
      title: c.title,
      active: c.id === activeId,
      status: c.status,
      pinned: c.isPinned ?? false,
    })
  }
  // 默认文件夹排在最后
  const entries = [...map.entries()]
  const defaultEntry = entries.find(([id]) => id === DEFAULT_PROJECT_ID)
  const rest = entries.filter(([id]) => id !== DEFAULT_PROJECT_ID)
  return [...rest, ...(defaultEntry ? [defaultEntry] : [])].map(([, p]) => p)
}
