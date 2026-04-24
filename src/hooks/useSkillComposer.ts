import { useCallback, useMemo, useState, type RefObject } from 'react'

import type { SkillInfo } from '@/lib/tauri'
import { useSkillStore } from '@/stores/skillStore'

interface UseSkillComposerArgs {
  input: string
  setInput: (value: string) => void
  textareaRef: RefObject<HTMLTextAreaElement | null>
}

export function useSkillComposer({
  input,
  setInput,
  textareaRef,
}: UseSkillComposerArgs) {
  const [showSkillPopover, setShowSkillPopover] = useState(false)
  const getSkillById = useSkillStore((s) => s.getById)

  const focusToEnd = useCallback((next: string) => {
    requestAnimationFrame(() => {
      const el = textareaRef.current
      if (!el) return
      el.focus()
      el.setSelectionRange(next.length, next.length)
    })
  }, [textareaRef])

  const slashMatch = useMemo(() => {
    if (!input.startsWith('/')) return null
    const rest = input.slice(1)
    const wsIdx = rest.search(/\s/)
    if (wsIdx === -1) {
      return { filter: rest, tail: '' }
    }
    return { filter: rest.slice(0, wsIdx), tail: rest.slice(wsIdx) }
  }, [input])

  const slashOpen = slashMatch !== null

  const handleSkillPick = useCallback((skillId: string) => {
    const skill = getSkillById(skillId)
    const trigger = skill?.triggerText || `/${skillId}`
    const next = trigger.endsWith(' ') ? trigger : `${trigger} `
    setInput(next)
    setShowSkillPopover(false)
    focusToEnd(next)
  }, [focusToEnd, getSkillById, setInput])

  const handleSlashSelect = useCallback((skill: SkillInfo) => {
    const tail = slashMatch?.tail ?? ''
    const next = tail ? `${skill.triggerText}${tail}` : skill.triggerText
    setInput(next)
    focusToEnd(next)
  }, [focusToEnd, setInput, slashMatch])

  const handleSlashClose = useCallback(() => {
    textareaRef.current?.focus()
  }, [textareaRef])

  return {
    showSkillPopover,
    setShowSkillPopover,
    slashMatch,
    slashOpen,
    handleSkillPick,
    handleSlashSelect,
    handleSlashClose,
  }
}
