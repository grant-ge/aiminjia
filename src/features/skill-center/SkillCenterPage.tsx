import { Puzzle } from 'lucide-react'

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'

export function SkillCenterPage() {
  return (
    <div className="flex h-full items-center justify-center p-6">
      <Card className="w-full max-w-3xl">
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-2xl">
            <Puzzle className="size-5 text-primary" />
            技能中心
          </CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground">技能中心骨架将在后续任务接入分类、推荐与安装流程。</p>
        </CardContent>
      </Card>
    </div>
  )
}
