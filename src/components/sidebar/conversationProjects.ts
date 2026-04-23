/**
 * Plan-A：把 useChat() 的 flat conversations 转成 project-grouped 结构。
 *
 * 项目分组当前由 `conversation.projectId` 决定，未提供则归到 "默认项目"。
 * （后端 Conversation 模型当前没有 projectId 字段，所以一切落到默认项目；
 *  待后续后端字段补齐后无需改本文件，只需提供 projectId/projectName。）
 */
import type { ConversationTreeProject } from './ConversationTree'

export interface RawConversation {
  id: string
  title: string
  projectId?: string | null
  projectName?: string | null
  loading?: boolean
}

export function groupConversationsByProject(
  conversations: RawConversation[],
  activeId: string | null,
): ConversationTreeProject[] {
  const map = new Map<string, ConversationTreeProject>()
  for (const c of conversations) {
    const projectId = c.projectId || 'default'
    const projectName = c.projectName || '默认项目'
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
  return [...map.values()]
}
