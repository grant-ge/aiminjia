/**
 * TypingIndicator — animated dots shown during AI streaming.
 * Based on visual-prototype-zh.html animation style.
 */

export function TypingIndicator() {
  return (
    <span className="inline-flex items-center gap-1">
      {[0, 1, 2].map((i) => (
        <span
          key={i}
          className="inline-block h-1.5 w-1.5 rounded-full"
          style={{
            background: 'var(--color-text-muted)',
            animation: `blink 1.2s infinite ${i * 0.2}s`,
          }}
        />
      ))}
    </span>
  )
}
