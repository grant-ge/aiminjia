import { Sparkles } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Textarea } from '@/components/ui/textarea'

const QUICK_PROMPTS = ['写一份周报', '分析销售数据', '帮我拆解执行计划']

export function HomePage() {
  return (
    <div className="flex h-full flex-col gap-6 overflow-auto px-8 py-8">
      <Card className="border-border bg-card">
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-2xl">
            <Sparkles className="size-5" />
            新任务
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <Textarea className="min-h-40 resize-none" placeholder="描述你现在要完成的任务..." />
          <div className="flex flex-wrap gap-2">
            {QUICK_PROMPTS.map((prompt) => (
              <Button key={prompt} variant="secondary">
                {prompt}
              </Button>
            ))}
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
