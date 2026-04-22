import { Timer } from 'lucide-react'

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'

export function SchedulesPage() {
  return (
    <div className="flex h-full items-center justify-center p-6">
      <Card className="w-full max-w-3xl">
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-2xl">
            <Timer className="size-5 text-primary" />
            定时任务
          </CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground">定时任务页面骨架已预留，后续任务会补齐列表与操作入口。</p>
        </CardContent>
      </Card>
    </div>
  )
}
