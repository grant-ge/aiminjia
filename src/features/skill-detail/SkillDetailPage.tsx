import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'

interface SkillDetailPageProps {
  skillId: string
}

export function SkillDetailPage({ skillId }: SkillDetailPageProps) {
  return (
    <div className="flex h-full items-center justify-center p-6">
      <Card className="w-full max-w-3xl">
        <CardHeader>
          <CardTitle className="text-2xl">技能详情</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground">当前查看技能：{skillId}</p>
        </CardContent>
      </Card>
    </div>
  )
}
