import { useCallback, useMemo, useState, type RefObject } from 'react'

import type { SkillInfo } from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'
import { useSkillStore } from '@/stores/skillStore'

interface UseSkillComposerArgs {
  input: string
  setInput: (value: string) => void
  textareaRef: RefObject<HTMLTextAreaElement | null>
  conversationId?: string | null
}

export function useSkillComposer({
  input,
  setInput,
  textareaRef,
  conversationId,
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

  const applySkillCommand = useCallback((skill: SkillInfo, tail: string) => {
    if (!conversationId) {
      const trigger = skill.triggerText || `/${skill.id}`
      const next = tail ? `${trigger}${tail}` : trigger
      setInput(next)
      focusToEnd(next)
      return
    }

    useChatStore.getState().setSelectedSkillCommand(conversationId, {
      id: skill.id,
      label: skill.displayName || skill.id,
      command: `/${skill.id}`,
    })
    const next = tail.trimStart()
    setInput(next)
    setShowSkillPopover(false)
    focusToEnd(next)
  }, [conversationId, focusToEnd, setInput])

  const handleInputChange = useCallback((value: string) => {
    if (!conversationId) {
      setInput(value)
      return
    }

    const match = value.match(/^\/([A-Za-z0-9][A-Za-z0-9_-]*)(\s+[\s\S]*)$/)
    if (!match) {
      setInput(value)
      return
    }

    const skill = getSkillById(match[1])
    if (!skill) {
      setInput(value)
      return
    }

    applySkillCommand(skill, match[2])
  }, [applySkillCommand, getSkillById, setInput])

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
    applySkillCommand(skill, tail)
  }, [applySkillCommand, slashMatch])

  const handleSlashClose = useCallback(() => {
    textareaRef.current?.focus()
  }, [textareaRef])

  return {
    showSkillPopover,
    setShowSkillPopover,
    slashMatch,
    slashOpen,
    handleInputChange,
    handleSkillPick,
    handleSlashSelect,
    handleSlashClose,
  }
}
