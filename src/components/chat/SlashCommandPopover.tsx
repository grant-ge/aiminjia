/**
 * SlashCommandPopover — skill picker triggered by typing "/" in the chat input.
 *
 * Renders a floating list of skills, filtered by the text after the slash.
 * Parent owns the open/close state and the filter text; this component
 * just renders + handles keyboard navigation within the visible list.
 *
 * Selection replaces the "/..." prefix in the parent input with the
 * selected skill's triggerText. The user can then edit / add context
 * before pressing Enter to send.
 */
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { usePluginStore } from '@/stores/pluginStore'
import type { SkillInfo } from '@/lib/tauri'

interface SlashCommandPopoverProps {
  /** Text typed AFTER the slash (may be empty). Used to filter skill list. */
  filterText: string
  /** Called when user picks a skill (Enter or click). */
  onSelect: (skill: SkillInfo) => void
  /** Called when popover should close (Escape, outside click, no matches + Enter). */
  onClose: () => void
}

/** Score skill against filter text. Higher = better match. 0 = no match. */
function scoreSkill(skill: SkillInfo, filter: string): number {
  if (!filter) return 1 // everything matches when no filter
  const f = filter.toLowerCase()

  // id exact prefix: highest
  if (skill.id.toLowerCase().startsWith(f)) return 100
  // displayName starts with: very high
  if (skill.displayName.toLowerCase().startsWith(f)) return 80
  if (skill.displayNameEn.toLowerCase().startsWith(f)) return 75
  // triggerText contains: medium-high
  if (skill.triggerText.toLowerCase().includes(f)) return 60
  // id contains: medium
  if (skill.id.toLowerCase().includes(f)) return 50
  // displayName contains: medium
  if (skill.displayName.toLowerCase().includes(f)) return 40
  if (skill.displayNameEn.toLowerCase().includes(f)) return 35
  // category/short description contain: low
  if (skill.category.toLowerCase().includes(f)) return 20
  if (skill.shortDescription.toLowerCase().includes(f)) return 15

  return 0
}

export function SlashCommandPopover({
  filterText,
  onSelect,
  onClose,
}: SlashCommandPopoverProps) {
  const { t, i18n } = useTranslation()
  const lang = i18n.language
  const skills = usePluginStore((s) => s.skills)
  const [selectionState, setSelectionState] = useState({ filterText: '', index: 0 })
  const listRef = useRef<HTMLUListElement>(null)

  // Filter + sort skills.
  //
  // Note: we intentionally do NOT apply persona.linkedCategories filtering
  // here (WelcomeScreen does). The `/` slash picker is an explicit escape
  // hatch: if the user typed it, they know what they're looking for, and
  // hiding skills based on the active persona would silently swallow matches.
  // (Reviewer flagged this 2026-04-16.)
  const visibleSkills = useMemo(() => {
    return skills
      .filter((s) => {
        // Exclude daily-assistant (the fallback one) and skills without icon
        if (s.id === 'daily-assistant' || !s.icon) return false
        // Score filter
        return scoreSkill(s, filterText) > 0
      })
      .map((s) => ({ skill: s, score: scoreSkill(s, filterText) }))
      .sort((a, b) => {
        // Primary sort: score desc
        if (b.score !== a.score) return b.score - a.score
        // Secondary: skill id alphabetic (stable)
        return a.skill.id.localeCompare(b.skill.id)
      })
      .map((x) => x.skill)
  }, [skills, filterText])

  const selectedIdx =
    selectionState.filterText === filterText ? selectionState.index : 0
  const clampedSelectedIdx = Math.min(
    selectedIdx,
    Math.max(0, visibleSkills.length - 1),
  )

  // Keyboard navigation — captured globally while popover is mounted
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'ArrowDown') {
        e.preventDefault()
        setSelectionState((current) => ({
          filterText,
          index: Math.min(
            (current.filterText === filterText ? current.index : 0) + 1,
            Math.max(0, visibleSkills.length - 1),
          ),
        }))
      } else if (e.key === 'ArrowUp') {
        e.preventDefault()
        setSelectionState((current) => ({
          filterText,
          index: Math.max(0, (current.filterText === filterText ? current.index : 0) - 1),
        }))
      } else if (e.key === 'Enter') {
        // Only intercept Enter if we have a match to select. If the list is
        // empty, let Enter fall through so the user can still send their
        // original "/" message as text.
        if (visibleSkills.length > 0) {
          e.preventDefault()
          const picked = visibleSkills[clampedSelectedIdx]
          if (picked) onSelect(picked)
        } else {
          // Nothing to pick — close and let parent handle the Enter normally
          onClose()
        }
      } else if (e.key === 'Escape') {
        e.preventDefault()
        onClose()
      } else if (e.key === 'Tab') {
        // Tab behaves like Enter-select without sending (same as pick)
        if (visibleSkills.length > 0) {
          e.preventDefault()
          const picked = visibleSkills[clampedSelectedIdx]
          if (picked) onSelect(picked)
        }
      }
    }
    // Capture so we can intercept Enter BEFORE the textarea's handler sends.
    window.addEventListener('keydown', handler, { capture: true })
    return () => window.removeEventListener('keydown', handler, { capture: true })
  }, [clampedSelectedIdx, filterText, onClose, onSelect, visibleSkills])

  // Scroll selected item into view as user navigates
  useEffect(() => {
    if (!listRef.current) return
    const selected = listRef.current.querySelector<HTMLElement>(
      `[data-slash-idx="${clampedSelectedIdx}"]`
    )
    if (selected) {
      selected.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
    }
  }, [clampedSelectedIdx])

  const empty = visibleSkills.length === 0

  return (
    <div
      className="animate-[fadeUp_0.15s_ease] absolute bottom-[calc(100%+8px)] left-0 z-50 w-[420px] max-w-full overflow-hidden rounded-lg"
      style={{
        background: 'var(--color-bg-card)',
        border: '1px solid var(--color-border-subtle)',
        boxShadow: '0 8px 24px rgba(0, 0, 0, 0.12)',
      }}
      onMouseDown={(e) => {
        // Prevent textarea from losing focus when clicking inside the popover
        e.preventDefault()
      }}
    >
      {/* Header hint */}
      <div
        className="px-3 py-2 text-xs font-medium"
        style={{
          color: 'var(--color-text-muted)',
          borderBottom: '1px solid var(--color-border-subtle)',
        }}
      >
        {empty
          ? t('inputBar.slashEmpty')
          : t('inputBar.slashFiltered', { count: visibleSkills.length })}
      </div>

      {/* Skill list */}
      {!empty && (
        <ul
          ref={listRef}
          className="max-h-[320px] list-none overflow-y-auto p-1"
          style={{ margin: 0 }}
        >
          {visibleSkills.map((skill, idx) => {
            const isSelected = idx === clampedSelectedIdx
            const name =
              lang === 'en-US' && skill.displayNameEn
                ? skill.displayNameEn
                : t(`skills.${skill.id}`, skill.displayName)
            const desc =
              lang === 'en-US' && skill.shortDescriptionEn
                ? skill.shortDescriptionEn
                : skill.shortDescription
            return (
              <li
                key={skill.id}
                data-slash-idx={idx}
                className="flex cursor-pointer items-start gap-2 rounded-md px-2 py-1.5 transition-colors"
                style={{
                  background: isSelected
                    ? 'var(--color-bg-card-hover)'
                    : 'transparent',
                }}
                onClick={() => onSelect(skill)}
                onMouseEnter={() => setSelectionState({ filterText, index: idx })}
              >
                <span className="mt-0.5 text-base leading-none">{skill.icon}</span>
                <div className="flex flex-1 flex-col gap-0.5 overflow-hidden">
                  <div className="flex items-center gap-2">
                    <span
                      className="truncate text-sm font-medium"
                      style={{ color: 'var(--color-text-primary)' }}
                    >
                      {name}
                    </span>
                    <span
                      className="shrink-0 rounded px-1.5 py-0 text-xs"
                      style={{
                        background: 'var(--color-bg-main)',
                        color: 'var(--color-text-muted)',
                      }}
                    >
                      {skill.category}
                    </span>
                  </div>
                  {desc && (
                    <span
                      className="truncate text-xs"
                      style={{ color: 'var(--color-text-muted)' }}
                    >
                      {desc}
                    </span>
                  )}
                </div>
              </li>
            )
          })}
        </ul>
      )}

      {/* Footer hint */}
      <div
        className="flex items-center justify-between px-3 py-1.5 text-xs"
        style={{
          color: 'var(--color-text-muted)',
          background: 'var(--color-bg-main)',
          borderTop: '1px solid var(--color-border-subtle)',
        }}
      >
        <span>↑↓ {t('inputBar.slashNav')}</span>
        <span>
          Enter {t('inputBar.slashSelect')} · Esc {t('inputBar.slashClose')}
        </span>
      </div>
    </div>
  )
}
