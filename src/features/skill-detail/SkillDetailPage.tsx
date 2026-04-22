import { Button } from '@/components/ui/button'
import { useChat } from '@/hooks/useChat'
import { useSkillStore } from '@/stores/skillStore'

interface SkillDetailPageProps {
  skillId: string
}

export function SkillDetailPage({ skillId }: SkillDetailPageProps) {
  const skill = useSkillStore((state) => state.getById(skillId))
  const { createConversationFromSkill } = useChat()

  if (!skill) {
    return <div className="p-8 text-sm text-muted-foreground">技能不存在或尚未加载。</div>
  }

  return (
    <div className="flex h-full flex-col gap-6 overflow-auto px-8 py-8">
      <div className="space-y-2">
        <h1 className="text-3xl font-semibold">{skill.displayName}</h1>
        <p className="max-w-3xl text-sm text-muted-foreground">{skill.description}</p>
      </div>
      <div className="rounded-lg border border-border bg-card p-6">
        <h2 className="text-base font-medium">工作流预览</h2>
        <ol className="mt-4 list-decimal space-y-2 pl-5 text-sm text-muted-foreground">
          <li>识别任务目标与上下文</li>
          <li>生成对应执行步骤</li>
          <li>回到会话中继续完成任务</li>
        </ol>
      </div>
      <div className="flex gap-3">
        <Button onClick={() => void createConversationFromSkill(skill.id)}>开始使用</Button>
        <Button variant="secondary">上传新版本</Button>
      </div>
    </div>
  )
}
