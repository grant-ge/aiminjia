import { useState } from 'react'
import { ChevronRight, Puzzle } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

export function SkillPopover() {
  const [open, setOpen] = useState(false)
  const skills = useSkillStore((state) => state.skills)
  const setRoute = useUiStore((state) => state.setRoute)

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button size="sm" variant="ghost">
          <Puzzle className="size-4" />
          技能
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-80 space-y-2">
        {skills.slice(0, 6).map((skill) => (
          <button
            key={skill.id}
            className="flex w-full items-center justify-between rounded-md px-3 py-2 text-left text-sm transition-colors hover:bg-accent"
            onClick={() => {
              setRoute({ kind: 'skill-detail', skillId: skill.id })
              setOpen(false)
            }}
            type="button"
          >
            <span>{skill.displayName}</span>
            <ChevronRight className="size-4 text-muted-foreground" />
          </button>
        ))}
        <Button className="w-full" variant="secondary" onClick={() => setRoute({ kind: 'skill-center' })}>
          去技能中心
        </Button>
      </PopoverContent>
    </Popover>
  )
}
