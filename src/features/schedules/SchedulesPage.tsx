import { CalendarClock, Sparkles } from 'lucide-react'

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'

const templates = ['日报汇总', '门店巡检', '周度复盘']

export function SchedulesPage() {
  return (
    <div className="flex h-full flex-col gap-6 overflow-auto px-8 py-8">
      <div>
        <h1 className="text-2xl font-semibold">定时任务</h1>
        <p className="text-sm text-muted-foreground">管理自动化任务与推荐模板。</p>
      </div>
      <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
        {templates.map((template) => (
          <Card key={template} className="border-border bg-card">
            <CardHeader>
              <CardTitle className="flex items-center gap-2 text-base">
                <Sparkles className="size-4 text-primary" />
                {template}
              </CardTitle>
            </CardHeader>
            <CardContent className="text-sm text-muted-foreground">模板即将开放，后续可一键创建周期任务。</CardContent>
          </Card>
        ))}
      </div>
      <Card className="border-border bg-card">
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <CalendarClock className="size-4 text-primary" />
            任务列表
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-[2fr_1fr_1fr] gap-3 text-sm font-medium text-foreground">
            <span>任务名称</span>
            <span>执行频率</span>
            <span>状态</span>
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
