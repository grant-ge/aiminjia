/**
 * 把 useChat() 的 flat conversations 转成 project-grouped 结构。
 *
 * 分组依据：conversation.workspaceName（后端从 authorized_workspace_store 注入）。
 * 未绑定工作目录的对话归到"默认文件夹"。
 */
import i18n from '@/i18n'

import type { ConversationTreeProject } from './ConversationTree'

export interface RawConversation {
  id: string
  title: string
  workspaceName?: string | null
  loading?: boolean
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
      loading: c.loading,
    })
  }
  // 默认文件夹排在最后
  const entries = [...map.entries()]
  const defaultEntry = entries.find(([id]) => id === DEFAULT_PROJECT_ID)
  const rest = entries.filter(([id]) => id !== DEFAULT_PROJECT_ID)
  return [...rest, ...(defaultEntry ? [defaultEntry] : [])].map(([, p]) => p)
}
