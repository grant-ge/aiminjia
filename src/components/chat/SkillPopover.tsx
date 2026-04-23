import { useState } from 'react'

import { SkillPopoverPanel } from '@/components/chat-scene/SkillPopoverPanel'
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

  const items = skills.map((s) => ({
    id: s.id,
    title: s.displayName,
    subtitle: s.shortDescription || s.description,
    source: s.source === 'builtin' ? '内置' : '已安装',
  }))

  return (
    <div className="absolute bottom-full left-10 z-30 mb-3">
      <SkillPopoverPanel items={items} onPick={handlePick} onClose={handleClose} />
    </div>
  )
}
