/**
 * OptionCards — selectable card grid for analysis choices.
 * Based on visual-prototype-zh.html .opt / .cards styles.
 */
import type { OptionGroup } from '@/types/message'

interface OptionCardsProps {
  group: OptionGroup
  onSelect?: (optionId: string) => void
}

export function OptionCards({ group, onSelect }: OptionCardsProps) {
  const gridCols = group.columns === 2 ? 'grid-cols-2' : 'grid-cols-3'

  return (
    <div className={`my-3 grid gap-3 ${gridCols}`}>
      {group.options.map((opt) => {
        const isSelected = group.selectedId === opt.id

        return (
          <button
            key={opt.id}
            type="button"
            className="relative cursor-pointer rounded-lg border-[1.5px] p-3.5 text-left transition-all duration-200"
            style={{
              background: isSelected
                ? 'var(--color-primary-subtle)'
                : 'var(--color-bg-card)',
              borderColor: isSelected
                ? 'var(--color-primary)'
                : 'var(--color-border)',
            }}
            onClick={() => onSelect?.(opt.id)}
          >
            {/* Tag */}
            {opt.tag && (
              <div
                className="mb-1.5 text-xs font-bold uppercase tracking-wide"
                style={{ color: opt.tagColor ?? 'var(--color-primary)' }}
              >
                {opt.tag}
              </div>
            )}

            <h4
              className="mb-1 text-base font-semibold"
              style={{ color: 'var(--color-text-primary)' }}
            >
              {opt.title}
            </h4>

            <p
              className="m-0 text-sm leading-snug"
              style={{ color: 'var(--color-text-muted)' }}
            >
              {opt.description}
            </p>

            {/* Selected checkmark */}
            {isSelected && (
              <div
                className="absolute right-2.5 top-2.5 flex h-[18px] w-[18px] items-center justify-center rounded-full"
                style={{ background: 'var(--color-primary)' }}
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="white">
                  <path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z" />
                </svg>
              </div>
            )}
          </button>
        )
      })}
    </div>
  )
}
