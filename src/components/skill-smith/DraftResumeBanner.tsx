/**
 * DraftResumeBanner — surfaces unfinished skill-smith drafts to the user.
 *
 * Mounted on WelcomeScreen and SkillsTab. Hidden when there are no drafts,
 * so it doesn't add noise for users who've never touched skill creation.
 *
 * Actions per draft:
 *  - "继续"  → send the skill-smith trigger text, which re-opens the skill.
 *             (M2: starts a fresh session; M3 will accept a draft_id arg and
 *             jump to the step matching the draft's stage.)
 *  - "丢弃"  → confirm dialog → discardSkillDraft() → reload list.
 *
 * Drafts older than 7 days are auto-cleaned on app startup by the Rust side
 * (see cleanup_expired_drafts in T2).
 */
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ask } from '@tauri-apps/plugin-dialog'
import { listSkillDrafts, discardSkillDraft } from '@/lib/tauri'
import type { DraftSummary } from '@/lib/tauri'
import { useChat } from '@/hooks/useChat'
import { useNotificationStore } from '@/stores/notificationStore'

const SKILL_SMITH_TRIGGER = '我想创建一个技能'

function formatRelative(tsSeconds: number, t: (k: string, o?: Record<string, unknown>) => string): string {
  const now = Math.floor(Date.now() / 1000)
  const delta = Math.max(0, now - tsSeconds)
  if (delta < 60) return t('skillSmith.banner.justNow')
  if (delta < 3600) return t('skillSmith.banner.minutesAgo', { n: Math.floor(delta / 60) })
  if (delta < 86400) return t('skillSmith.banner.hoursAgo', { n: Math.floor(delta / 3600) })
  return t('skillSmith.banner.daysAgo', { n: Math.floor(delta / 86400) })
}

interface DraftResumeBannerProps {
  /**
   * Optional callback invoked after the user clicks "Resume" and the
   * skill-smith session is kicked off. Useful when the banner lives
   * inside a modal (e.g. SkillsTab inside SettingsModal) — pass the
   * modal's close handler so we surface the chat automatically.
   */
  onAfterResume?: () => void
}

export function DraftResumeBanner({ onAfterResume }: DraftResumeBannerProps = {}) {
  const { t } = useTranslation()
  const [drafts, setDrafts] = useState<DraftSummary[]>([])
  const [loaded, setLoaded] = useState(false)
  const { sendUserMessage } = useChat()
  const pushNotification = useNotificationStore((s) => s.push)

  const reload = useCallback(async () => {
    try {
      const list = await listSkillDrafts()
      setDrafts(list)
    } catch {
      setDrafts([])
    } finally {
      setLoaded(true)
    }
  }, [])

  useEffect(() => {
    void reload()
  }, [reload])

  const handleResume = useCallback(
    (draftId: string) => {
      sendUserMessage(`${SKILL_SMITH_TRIGGER} resume:${draftId}`)
      onAfterResume?.()
    },
    [sendUserMessage, onAfterResume],
  )

  const handleDiscard = useCallback(
    async (draft: DraftSummary) => {
      const name = draft.skillName ?? t('skillSmith.banner.unnamed')
      // Wrap the entire flow in try/catch so any failure (ask() throwing,
      // permission missing, IPC error) surfaces as a user-visible toast
      // instead of a silent no-op. Without this, a thrown ask() left the
      // user staring at an unresponsive button.
      try {
        // eslint-disable-next-line no-console
        console.log('[DraftResumeBanner] discard clicked', draft.draftId)
        const confirmed = await ask(t('skillSmith.banner.discardConfirm', { name }), {
          title: t('skillSmith.banner.discardTitle'),
          kind: 'warning',
        })
        // eslint-disable-next-line no-console
        console.log('[DraftResumeBanner] ask resolved', { confirmed })
        if (!confirmed) return

        await discardSkillDraft(draft.draftId)
        await reload()
      } catch (err) {
        // eslint-disable-next-line no-console
        console.error('[DraftResumeBanner] discard failed', err)
        const msg = err instanceof Error ? err.message : String(err)
        pushNotification({
          level: 'error',
          title: t('skillSmith.banner.discardTitle'),
          message: msg || 'discard failed',
          actions: [],
          dismissible: true,
          autoHide: 5,
          context: 'toast',
        })
        // Best-effort reload — the draft may have been deleted despite the
        // error path firing (e.g. ask threw after IPC succeeded).
        await reload().catch(() => {})
      }
    },
    [reload, t, pushNotification],
  )

  if (!loaded || drafts.length === 0) return null

  return (
    <div
      className="animate-[fadeUp_0.3s_ease] w-full max-w-[640px] px-4"
      role="region"
      aria-label={t('skillSmith.banner.title', { count: drafts.length })}
    >
      <div
        className="mb-2 rounded-lg px-4 py-3"
        style={{
          background: 'var(--color-bg-elevated)',
          border: '1px solid var(--color-border-subtle)',
        }}
      >
        <div
          className="mb-2 flex items-center gap-2 text-sm font-medium"
          style={{ color: 'var(--color-text-primary)' }}
        >
          <span className="text-base leading-none">🛠️</span>
          <span>{t('skillSmith.banner.title', { count: drafts.length })}</span>
        </div>
        <ul className="flex flex-col gap-1.5">
          {drafts.map((d) => {
            const name = d.skillName ?? t('skillSmith.banner.unnamed')
            const relative = formatRelative(d.lastModified, t)
            return (
              <li
                key={d.draftId}
                className="flex items-center gap-2 text-xs"
                style={{ color: 'var(--color-text-muted)' }}
              >
                <span
                  className="flex-1 truncate"
                  style={{ color: 'var(--color-text-primary)' }}
                  title={`${name} · ${d.stage} · ${relative}`}
                >
                  「{name}」
                </span>
                <span className="shrink-0">{relative}</span>
                <button
                  type="button"
                  className="shrink-0 rounded px-2 py-0.5 transition-colors"
                  style={{
                    color: 'var(--color-accent)',
                    background: 'transparent',
                  }}
                  onClick={() => handleResume(d.draftId)}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.background = 'var(--color-bg-hover)'
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.background = 'transparent'
                  }}
                >
                  {t('skillSmith.banner.resume')}
                </button>
                <button
                  type="button"
                  className="shrink-0 rounded px-2 py-0.5 transition-colors"
                  style={{
                    color: 'var(--color-text-muted)',
                    background: 'transparent',
                  }}
                  onClick={() => handleDiscard(d)}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.background = 'var(--color-bg-hover)'
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.background = 'transparent'
                  }}
                >
                  {t('skillSmith.banner.discard')}
                </button>
              </li>
            )
          })}
        </ul>
      </div>
    </div>
  )
}
