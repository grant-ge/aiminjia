/**
 * WelcomeScreen — greeting + general mode entry + flat skill grid.
 * Shows only skills linked to the active persona (or all if none linked).
 * Persona switching is handled in Sidebar.
 */
import { usePluginStore } from '@/stores/pluginStore'
import { usePersonaStore } from '@/stores/personaStore'
import { useChat } from '@/hooks/useChat'

export function WelcomeScreen() {
  const skills = usePluginStore((s) => s.skills)
  const activePersona = usePersonaStore((s) => s.activePersona)
  const { sendUserMessage } = useChat()

  // Linked categories from active persona (default: show all)
  const linkedCategories = activePersona?.linkedCategories ?? []
  const hasLinked = linkedCategories.length > 0

  // Filter skills: only linked categories, exclude daily-assistant, must have icon
  const displaySkills = skills.filter((s) => {
    if (s.id === 'daily-assistant' || !s.icon) return false
    if (!hasLinked) return true
    return linkedCategories.includes(s.category || 'general')
  })

  const handleSkillClick = (triggerText: string) => {
    if (triggerText) {
      sendUserMessage(triggerText)
    }
  }

  const handleGeneralMode = () => {
    // Focus the chat input textarea
    const textarea = document.querySelector<HTMLTextAreaElement>(
      'textarea[placeholder]'
    )
    textarea?.focus()
  }

  // Greeting follows persona
  const greeting = activePersona
    ? `你好！我是 AI小家 · ${activePersona.name}`
    : '你好！我是 AI小家'
  const subtitle = activePersona?.description || '你的智能工作助手'

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
        {activePersona?.icon || '家'}
      </div>
      <h2
        className="mb-1 text-lg font-semibold"
        style={{ color: 'var(--color-text-primary)' }}
      >
        {greeting}
      </h2>
      <p
        className="mb-4 max-w-md text-center text-sm"
        style={{ color: 'var(--color-text-muted)' }}
      >
        {subtitle}
      </p>

      {/* General mode entry */}
      <div className="w-full max-w-[640px] px-4">
        <button
          type="button"
          className="flex w-full items-center gap-3 rounded-lg px-4 py-3 text-left transition-all duration-150 cursor-pointer"
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
          onClick={handleGeneralMode}
        >
          <span className="text-xl leading-none">💬</span>
          <div>
            <span
              className="text-sm font-medium"
              style={{ color: 'var(--color-text-primary)' }}
            >
              通用模式
            </span>
            <p
              className="mt-0.5 text-xs"
              style={{ color: 'var(--color-text-muted)' }}
            >
              直接对话，不使用专业技能
            </p>
          </div>
        </button>
      </div>

      {/* Flat skill grid */}
      {displaySkills.length > 0 && (
        <div className="mt-4 w-full max-w-[640px] px-4">
          <div className="grid grid-cols-3 gap-2.5">
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
        </div>
      )}

      <p
        className="mt-5 text-xs"
        style={{ color: 'var(--color-text-muted)' }}
      >
        也可以直接问我任何工作相关问题
      </p>
    </div>
  )
}
