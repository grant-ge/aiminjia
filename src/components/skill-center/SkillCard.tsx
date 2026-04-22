import { ArrowRight, BadgeCheck } from 'lucide-react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader, CardTitle } from '@/components/ui/card'
import type { SkillInfo } from '@/lib/tauri'

interface SkillCardProps {
  skill: SkillInfo
  onOpen: () => void
  onUse: () => void
}

export function SkillCard({ skill, onOpen, onUse }: SkillCardProps) {
  return (
    <Card className="flex h-full flex-col border-border bg-card transition-colors hover:border-primary/40">
      <CardHeader className="space-y-3">
        <div className="flex items-start justify-between gap-3">
          <CardTitle className="text-base">{skill.displayName}</CardTitle>
          <Badge variant={skill.source === 'builtin' ? 'secondary' : 'outline'}>
            {skill.source === 'builtin' ? '内置' : '已安装'}
          </Badge>
        </div>
        <p className="text-sm text-muted-foreground">{skill.shortDescription || skill.description}</p>
      </CardHeader>
      <CardContent className="flex-1">
        {skill.hasWorkflow ? (
          <div className="flex items-center gap-2 text-sm text-primary">
            <BadgeCheck className="size-4" />
            支持工作流
          </div>
        ) : null}
      </CardContent>
      <CardFooter className="flex gap-2">
        <Button className="flex-1" variant="secondary" onClick={onOpen}>
          查看详情
        </Button>
        <Button className="flex-1" onClick={onUse}>
          开始使用
          <ArrowRight className="size-4" />
        </Button>
      </CardFooter>
    </Card>
  )
}
