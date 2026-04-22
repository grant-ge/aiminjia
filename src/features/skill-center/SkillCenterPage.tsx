import { useMemo, useState } from 'react'

import { Plus, Store } from 'lucide-react'

import { SkillCard } from '@/components/skill-center/SkillCard'
import { Button } from '@/components/ui/button'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { SKILL_CATEGORIES, type SkillCategoryId } from '@/data/skill-categories'
import { useChat } from '@/hooks/useChat'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

import { SkillMarketModal } from './SkillMarketModal'
import { SkillUploadModal } from './SkillUploadModal'

export function SkillCenterPage() {
  const [category, setCategory] = useState<SkillCategoryId>('recommended')
  const [marketOpen, setMarketOpen] = useState(false)
  const [uploadOpen, setUploadOpen] = useState(false)
  const listByCategory = useSkillStore((state) => state.listByCategory)
  const setRoute = useUiStore((state) => state.setRoute)
  const { createConversationFromSkill } = useChat()

  const categories = useMemo(
    () => [{ id: 'recommended' as const, name: '为你推荐' }, ...SKILL_CATEGORIES],
    [],
  )
  const skills = listByCategory(category)

  return (
    <div className="flex h-full flex-col gap-6 overflow-auto px-8 py-8">
      <div className="flex items-center justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold">技能中心</h1>
          <p className="text-sm text-muted-foreground">浏览、试用和管理你的技能。</p>
        </div>
        <div className="flex gap-2">
          <Button variant="secondary" onClick={() => setMarketOpen(true)}>
            <Store className="size-4" />
            技能市场
          </Button>
          <Button onClick={() => setUploadOpen(true)}>
            <Plus className="size-4" />
            上传技能
          </Button>
        </div>
      </div>
      <Tabs value={category} onValueChange={(value) => setCategory(value as SkillCategoryId)}>
        <TabsList className="flex w-full justify-start overflow-auto">
          {categories.map((item) => (
            <TabsTrigger key={item.id} value={item.id}>
              {item.name}
            </TabsTrigger>
          ))}
        </TabsList>
      </Tabs>
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        {skills.map((skill) => (
          <SkillCard
            key={skill.id}
            skill={skill}
            onOpen={() => setRoute({ kind: 'skill-detail', skillId: skill.id })}
            onUse={() => void createConversationFromSkill(skill.id)}
          />
        ))}
      </div>
      <SkillMarketModal open={marketOpen} onOpenChange={setMarketOpen} />
      <SkillUploadModal open={uploadOpen} onOpenChange={setUploadOpen} />
    </div>
  )
}
