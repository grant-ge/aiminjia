/**
 * WelcomeScreen — greeting with brand identity.
 * Simple, clean layout: avatar + greeting + subtitle.
 */

export function WelcomeScreen() {
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
        className="mb-6 text-sm"
        style={{ color: 'var(--color-text-muted)' }}
      >
        组织，薪酬等HR相关的任何问题，你都可以找我聊
      </p>
    </div>
  )
}
