import { Sparkles } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { useChat } from '@/hooks/useChat'

export function HomePage() {
  const { createNewConversation } = useChat()

  return (
    <div className="flex h-full items-center justify-center p-6">
      <Card className="w-full max-w-2xl">
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-2xl">
            <Sparkles className="size-5 text-primary" />
            新任务
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <p className="text-sm text-muted-foreground">Skill-First 首页骨架已就位，下一步将接入技能中心与导航体验。</p>
          <Button onClick={() => void createNewConversation()}>开始新对话</Button>
        </CardContent>
      </Card>
    </div>
  )
}
