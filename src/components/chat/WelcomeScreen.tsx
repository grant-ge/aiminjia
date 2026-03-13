/**
 * WelcomeScreen — greeting + skill discovery cards.
 * Shows available HR skills as clickable cards that trigger analysis.
 */
import { usePluginStore } from '@/stores/pluginStore'
import { useChat } from '@/hooks/useChat'

export function WelcomeScreen() {
  const skills = usePluginStore((s) => s.skills)
  const { sendUserMessage } = useChat()

  const displaySkills = skills.filter(
    (s) => s.id !== 'daily-assistant' && s.icon
  )

  const handleSkillClick = (triggerText: string) => {
    if (triggerText) {
      sendUserMessage(triggerText)
    }
  }

  return (
    <div className="animate-[fadeUp_0.3s_ease] flex flex-col items-center pt-12">
      {/* Avatar */}
      <div
        className="mb-1.5 flex h-8 w-8 items-center justify-center rounded-full text-sm font-bold"
        style={{
          background: 'var(--color-accent)',
          color: 'var(--color-text-on-accent)',
        }}
      >
        家
      </div>
      <h2
        className="mb-1 text-lg font-semibold"
        style={{ color: 'var(--color-text-primary)' }}
      >
        你好！我是 AI小家
      </h2>
      <p
        className="mb-1 text-sm"
        style={{ color: 'var(--color-text-muted)' }}
      >
        专业的 HR 数据分析与管理顾问
      </p>

      {/* Skill cards grid */}
      {displaySkills.length > 0 && (
        <div className="mt-6 grid w-full max-w-[640px] grid-cols-3 gap-2.5 px-4">
          {displaySkills.map((skill) => (
            <button
              key={skill.id}
              type="button"
              className="flex flex-col items-center gap-1.5 rounded-lg px-3 py-3.5 text-center transition-all duration-150 hover:-translate-y-0.5 cursor-pointer"
              style={{
                background: 'var(--color-bg-elevated)',
                border: '1px solid var(--color-border-subtle)',
              }}
              onMouseEnter={(e) => {
                e.currentTarget.style.borderColor = 'var(--color-accent)'
                e.currentTarget.style.boxShadow = '0 2px 8px rgba(0,0,0,0.06)'
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.borderColor = 'var(--color-border-subtle)'
                e.currentTarget.style.boxShadow = 'none'
              }}
              onClick={() => handleSkillClick(skill.triggerText)}
            >
              <span className="text-xl leading-none">{skill.icon}</span>
              <span
                className="text-xs font-medium leading-tight"
                style={{ color: 'var(--color-text-primary)' }}
              >
                {skill.displayName}
              </span>
            </button>
          ))}
        </div>
      )}

      <p
        className="mt-5 text-xs"
        style={{ color: 'var(--color-text-muted)' }}
      >
        也可以直接问我任何 HR 相关问题
      </p>
    </div>
  )
}
