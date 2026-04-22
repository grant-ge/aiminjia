import { MessageSquare } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { useChat } from '@/hooks/useChat'

export function ConversationTree() {
  const { conversations, activeConversationId, switchConversation } = useChat()

  if (conversations.length === 0) {
    return (
      <div className="px-1 py-6 text-sm text-muted-foreground">还没有历史任务</div>
    )
  }

  return (
    <div className="space-y-1 pb-3">
      {conversations.map((conversation) => (
        <Button
          key={conversation.id}
          className="h-auto w-full justify-start px-3 py-2 text-left"
          variant={activeConversationId === conversation.id ? 'secondary' : 'ghost'}
          onClick={() => void switchConversation(conversation.id)}
        >
          <MessageSquare className="mt-0.5 size-4 shrink-0" />
          <span className="truncate">{conversation.title}</span>
        </Button>
      ))}
    </div>
  )
}
