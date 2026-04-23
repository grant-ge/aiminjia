import { useMemo, useState } from 'react'
import { Sparkles } from 'lucide-react'

import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { SkillCard } from '@/components/skills/SkillCard'
import { SkillCategoryBar } from '@/components/skills/SkillCategoryBar'
import { SkillHotSection } from '@/components/skills/SkillHotSection'
import { SkillOfficeSection } from '@/components/skills/SkillOfficeSection'
import { Button } from '@/components/ui/button'
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
  const listByCategory = useSkillStore((s) => s.listByCategory)
  const setRoute = useUiStore((s) => s.setRoute)
  const { createConversationFromSkill } = useChat()

  const officeCategories = useMemo(
    () => SKILL_CATEGORIES.map((c) => ({ key: c.id, label: c.name })),
    [],
  )
  const recommended = listByCategory('recommended')
  const officeSkills = listByCategory(category)

  return (
    <PageSectionShell
      topBar={
        <PageTopBar
          variant="default"
          trailing={
            <div className="flex items-center gap-2">
              <Button variant="secondary" onClick={() => setMarketOpen(true)}>
                技能市场
              </Button>
              <Button onClick={() => setUploadOpen(true)}>上传技能</Button>
            </div>
          }
        />
      }
      padding="px-7 pt-6 pb-8"
      gap="gap-5"
    >
      <SkillHotSection>
        {recommended.map((skill) => (
          <SkillCard
            key={skill.id}
            title={skill.displayName}
            desc={skill.shortDescription || skill.description}
            iconNode={<Sparkles className="h-4 w-4 text-primary" />}
            onOpen={() => setRoute({ kind: 'skill-detail', skillId: skill.id })}
            onUse={() => void createConversationFromSkill(skill.id)}
          />
        ))}
      </SkillHotSection>
      <SkillOfficeSection
        categoryBar={
          <SkillCategoryBar
            items={officeCategories}
            activeKey={category}
            onSelect={(key) => setCategory(key as SkillCategoryId)}
          />
        }
      >
        {officeSkills.map((skill) => (
          <SkillCard
            key={skill.id}
            title={skill.displayName}
            desc={skill.shortDescription || skill.description}
            iconNode={<Sparkles className="h-4 w-4 text-primary" />}
            onOpen={() => setRoute({ kind: 'skill-detail', skillId: skill.id })}
            onUse={() => void createConversationFromSkill(skill.id)}
          />
        ))}
      </SkillOfficeSection>
      <SkillMarketModal open={marketOpen} onOpenChange={setMarketOpen} />
      <SkillUploadModal open={uploadOpen} onOpenChange={setUploadOpen} />
    </PageSectionShell>
  )
}
