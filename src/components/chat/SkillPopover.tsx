import { useState } from 'react'

import { SkillPopoverPanel } from '@/components/chat-scene/SkillPopoverPanel'
import { localizeSkill } from '@/lib/skillLocalization'
import { useSkillStore } from '@/stores/skillStore'

interface SkillPopoverProps {
  open?: boolean
  onPick?: (skillId: string) => void
  onClose?: () => void
}

export function SkillPopover({ open: openProp, onPick, onClose }: SkillPopoverProps) {
  const [internalOpen, setInternalOpen] = useState(false)
  const skills = useSkillStore((s) => s.skills)

  // Support both controlled (open/onClose) and uncontrolled (internal) usage
  const isOpen = openProp !== undefined ? openProp : internalOpen

  const handleClose = () => {
    if (onClose) {
      onClose()
    } else {
      setInternalOpen(false)
    }
  }

  const handlePick = (skillId: string) => {
    if (onPick) {
      onPick(skillId)
    }
    handleClose()
  }

  if (!isOpen) return null

  const items = skills.map((s) => {
    const localized = localizeSkill(s)
    return {
      id: s.id,
      title: localized.name,
      subtitle: localized.description,
      icon: s.icon || undefined,
      category: s.category || undefined,
    }
  })

  return <SkillPopoverPanel items={items} onPick={handlePick} onClose={handleClose} />
}
